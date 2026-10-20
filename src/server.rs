use std::collections::HashMap;

use anyhow::Result;
use jrsonnet_formatter::{FormatOptions, format};
use jrsonnet_rowan_parser::{
	AstNode, LocatedSyntaxError, SyntaxKind, SyntaxNode, SyntaxToken, parse, rowan::TextRange,
};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
	Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
	DidOpenTextDocumentParams, DocumentFormattingParams, GotoDefinitionParams,
	GotoDefinitionResponse, InitializeParams, Location, OneOf, Position, PrepareRenameResponse,
	PublishDiagnosticsParams, ReferenceParams, RenameOptions, RenameParams, ServerCapabilities,
	TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
	WorkspaceEdit,
	notification::{
		DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
		PublishDiagnostics,
	},
	request::{
		Formatting, GotoDefinition, PrepareRenameRequest, References, Rename, Request as _,
	},
};

use crate::{
	line_index::LineIndex,
	scope::{self, Decl, Resolution},
};

struct Document {
	text: String,
	index: LineIndex,
	root: SyntaxNode,
	errors: Vec<LocatedSyntaxError>,
}

impl Document {
	fn new(text: String) -> Self {
		let index = LineIndex::new(&text);
		let (root, errors) = parse(&text);
		Self {
			text,
			index,
			root: root.syntax().clone(),
			errors,
		}
	}

	fn range(&self, r: TextRange) -> lsp_types::Range {
		self.index
			.range(&self.text, usize::from(r.start()), usize::from(r.end()))
	}

	/// The identifier token under the cursor, if any.
	fn ident_at(&self, pos: Position) -> Option<SyntaxToken> {
		let offset = self.index.offset(&self.text, pos)?;
		self.root
			.token_at_offset(u32::try_from(offset).ok()?.into())
			.find(|t| t.kind() == SyntaxKind::IDENT)
	}

	/// The local declaration the identifier at `pos` refers to or declares.
	fn decl_at(&self, pos: Position) -> Option<Decl> {
		match scope::decl_at(&self.root, &self.ident_at(pos)?)? {
			Resolution::Local(d) => Some(d),
			Resolution::Std | Resolution::Unresolved => None,
		}
	}
}

pub fn run() -> Result<()> {
	let (connection, io_threads) = Connection::stdio();
	let capabilities = serde_json::to_value(ServerCapabilities {
		text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
		document_formatting_provider: Some(OneOf::Left(true)),
		definition_provider: Some(OneOf::Left(true)),
		references_provider: Some(OneOf::Left(true)),
		rename_provider: Some(OneOf::Right(RenameOptions {
			prepare_provider: Some(true),
			work_done_progress_options: Default::default(),
		})),
		..Default::default()
	})?;
	let init = connection.initialize(capabilities)?;
	let _params: InitializeParams = serde_json::from_value(init)?;

	Server {
		connection: &connection,
		docs: HashMap::new(),
	}
	.main_loop()?;
	// The writer thread exits only once the sender side is dropped.
	drop(connection);
	io_threads.join()?;
	Ok(())
}

struct Server<'a> {
	connection: &'a Connection,
	docs: HashMap<Uri, Document>,
}

impl Server<'_> {
	fn main_loop(&mut self) -> Result<()> {
		// Clone so `self` can be borrowed mutably while handling messages.
		let receiver = self.connection.receiver.clone();
		for msg in &receiver {
			match msg {
				Message::Request(req) => {
					if self.connection.handle_shutdown(&req)? {
						return Ok(());
					}
					self.on_request(req)?;
				}
				Message::Notification(not) => self.on_notification(not)?,
				Message::Response(_) => {}
			}
		}
		Ok(())
	}

	fn on_request(&mut self, req: Request) -> Result<()> {
		let id = req.id.clone();
		let resp = match self.handle_request(req) {
			Ok(result) => Response {
				id,
				result: Some(result),
				error: None,
			},
			Err((code, message)) => Response::new_err(id, code as i32, message),
		};
		self.connection.sender.send(resp.into())?;
		Ok(())
	}

	fn handle_request(
		&mut self,
		req: Request,
	) -> std::result::Result<serde_json::Value, (ErrorCode, String)> {
		fn params<T: serde::de::DeserializeOwned>(
			req: &Request,
		) -> std::result::Result<T, (ErrorCode, String)> {
			serde_json::from_value(req.params.clone())
				.map_err(|e| (ErrorCode::InvalidParams, e.to_string()))
		}
		fn json<T: serde::Serialize>(v: T) -> std::result::Result<serde_json::Value, (ErrorCode, String)> {
			serde_json::to_value(v).map_err(|e| (ErrorCode::InternalError, e.to_string()))
		}

		match req.method.as_str() {
			Formatting::METHOD => json(self.formatting(&params(&req)?)),
			GotoDefinition::METHOD => json(self.definition(&params(&req)?)),
			References::METHOD => json(self.references(&params(&req)?)),
			PrepareRenameRequest::METHOD => json(self.prepare_rename(&params(&req)?)),
			Rename::METHOD => self.rename(&params(&req)?).and_then(json),
			m => Err((ErrorCode::MethodNotFound, format!("unsupported request {m}"))),
		}
	}

	fn on_notification(&mut self, not: Notification) -> Result<()> {
		match not.method.as_str() {
			DidOpenTextDocument::METHOD => {
				let p: DidOpenTextDocumentParams = serde_json::from_value(not.params)?;
				self.update(p.text_document.uri, p.text_document.text)?;
			}
			DidChangeTextDocument::METHOD => {
				let p: DidChangeTextDocumentParams = serde_json::from_value(not.params)?;
				// Full sync: the last change carries the whole text.
				if let Some(change) = p.content_changes.into_iter().last() {
					self.update(p.text_document.uri, change.text)?;
				}
			}
			DidCloseTextDocument::METHOD => {
				let p: DidCloseTextDocumentParams = serde_json::from_value(not.params)?;
				self.docs.remove(&p.text_document.uri);
				self.publish(p.text_document.uri, vec![])?;
			}
			_ => {}
		}
		Ok(())
	}

	fn update(&mut self, uri: Uri, text: String) -> Result<()> {
		let doc = Document::new(text);
		let diagnostics = diagnostics(&doc);
		self.docs.insert(uri.clone(), doc);
		self.publish(uri, diagnostics)
	}

	fn publish(&self, uri: Uri, diagnostics: Vec<Diagnostic>) -> Result<()> {
		let params = PublishDiagnosticsParams {
			uri,
			diagnostics,
			version: None,
		};
		self.connection
			.sender
			.send(Notification::new(PublishDiagnostics::METHOD.to_owned(), params).into())?;
		Ok(())
	}

	fn formatting(&self, params: &DocumentFormattingParams) -> Option<Vec<TextEdit>> {
		let doc = self.docs.get(&params.text_document.uri)?;
		let opts = FormatOptions {
			indent: params.options.tab_size.try_into().unwrap_or(4),
			use_tabs: !params.options.insert_spaces,
			..FormatOptions::new()
		};
		// The formatter refuses input with syntax errors; report no edits then.
		let mut formatted = format(&doc.text, &opts).ok()?;
		formatted.truncate(formatted.trim_end().len());
		formatted.push('\n');
		if formatted == doc.text {
			return Some(vec![]);
		}
		Some(vec![TextEdit::new(doc.index.full_range(&doc.text), formatted)])
	}

	fn definition(&self, params: &GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
		let pos = &params.text_document_position_params;
		let doc = self.docs.get(&pos.text_document.uri)?;
		let decl = doc.decl_at(pos.position)?;
		Some(GotoDefinitionResponse::Scalar(Location::new(
			pos.text_document.uri.clone(),
			doc.range(decl.range),
		)))
	}

	fn references(&self, params: &ReferenceParams) -> Option<Vec<Location>> {
		let pos = &params.text_document_position;
		let doc = self.docs.get(&pos.text_document.uri)?;
		let decl = doc.decl_at(pos.position)?;
		let mut ranges = scope::references(&doc.root, &decl);
		if params.context.include_declaration {
			ranges.insert(0, decl.range);
		}
		Some(
			ranges
				.into_iter()
				.map(|r| Location::new(pos.text_document.uri.clone(), doc.range(r)))
				.collect(),
		)
	}

	fn prepare_rename(&self, params: &TextDocumentPositionParams) -> Option<PrepareRenameResponse> {
		let doc = self.docs.get(&params.text_document.uri)?;
		// Only names that resolve to a local can be renamed: not `std`, not fields.
		doc.decl_at(params.position)?;
		let tok = doc.ident_at(params.position)?;
		Some(PrepareRenameResponse::Range(doc.range(tok.text_range())))
	}

	fn rename(&self, params: &RenameParams) -> std::result::Result<Option<WorkspaceEdit>, (ErrorCode, String)> {
		let pos = &params.text_document_position;
		if !scope::is_valid_ident(&params.new_name) {
			return Err((
				ErrorCode::InvalidParams,
				format!("`{}` is not a valid identifier", params.new_name),
			));
		}
		let Some(doc) = self.docs.get(&pos.text_document.uri) else {
			return Ok(None);
		};
		let Some(decl) = doc.decl_at(pos.position) else {
			return Ok(None);
		};
		let edits = std::iter::once(decl.range)
			.chain(scope::references(&doc.root, &decl))
			.map(|r| TextEdit::new(doc.range(r), params.new_name.clone()))
			.collect();
		Ok(Some(WorkspaceEdit {
			changes: Some(HashMap::from([(pos.text_document.uri.clone(), edits)])),
			..Default::default()
		}))
	}
}

fn diagnostics(doc: &Document) -> Vec<Diagnostic> {
	let syntax = doc.errors.iter().map(|e| Diagnostic {
		range: doc.range(e.range),
		// TODO: SyntaxError isn't exported upstream, so `Hint`s can't be told apart.
		severity: Some(DiagnosticSeverity::ERROR),
		source: Some("jsonnet-lsp".to_owned()),
		message: e.error.to_string(),
		..Default::default()
	});
	// Name resolution on a tree with syntax errors reports too much noise.
	let unresolved = doc
		.errors
		.is_empty()
		.then(|| scope::uses(&doc.root))
		.into_iter()
		.flatten()
		.filter(|(_, r)| *r == Resolution::Unresolved)
		.map(|(name, _)| Diagnostic {
			range: doc.range(name.range),
			severity: Some(DiagnosticSeverity::ERROR),
			source: Some("jsonnet-lsp".to_owned()),
			message: format!("undefined variable `{}`", name.name),
			..Default::default()
		});
	syntax.chain(unresolved).collect()
}
