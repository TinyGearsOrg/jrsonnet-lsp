use std::{
	collections::HashMap,
	path::{Path, PathBuf},
};

use anyhow::Result;
use jrsonnet_formatter::{FormatOptions, format};
use jrsonnet_rowan_parser::{
	AstNode, LocatedSyntaxError, SyntaxKind, SyntaxNode, SyntaxToken, parse, rowan::TextRange,
};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
	Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
	DocumentLink, DocumentLinkOptions, DocumentLinkParams,
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
		DocumentLinkRequest, Formatting, GotoDefinition, PrepareRenameRequest, References, Rename,
		Request as _,
	},
};

use crate::{
	imports,
	line_index::LineIndex,
	scope::{self, Decl, Resolution},
	uri,
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

	/// The import whose path string is under the cursor.
	fn import_at(&self, pos: Position) -> Option<imports::Import> {
		let offset = u32::try_from(self.index.offset(&self.text, pos)?).ok()?;
		imports::imports(&self.root)
			.into_iter()
			.find(|i| i.range.contains_inclusive(offset.into()))
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
		document_link_provider: Some(DocumentLinkOptions {
			resolve_provider: Some(false),
			work_done_progress_options: Default::default(),
		}),
		..Default::default()
	})?;
	let init = connection.initialize(capabilities)?;
	let params: InitializeParams = serde_json::from_value(init)?;

	Server {
		connection: &connection,
		docs: HashMap::new(),
		search: search_paths(&params),
	}
	.main_loop()?;
	// The writer thread exits only once the sender side is dropped.
	drop(connection);
	io_threads.join()?;
	Ok(())
}

/// Directories imports are looked up in after the importing file's own directory:
/// the workspace roots and their `vendor` (jsonnet-bundler) and `lib` directories, then any
/// `jpath` given in the client's initialization options (relative to the first root).
fn search_paths(params: &InitializeParams) -> Vec<PathBuf> {
	#[allow(deprecated)]
	let roots: Vec<PathBuf> = match &params.workspace_folders {
		Some(folders) => folders.iter().filter_map(|f| uri::to_path(&f.uri)).collect(),
		None => params.root_uri.iter().filter_map(uri::to_path).collect(),
	};
	let mut out = vec![];
	for root in &roots {
		out.extend([root.clone(), root.join("vendor"), root.join("lib")]);
	}
	let jpath = params
		.initialization_options
		.as_ref()
		.and_then(|o| o.get("jpath"))
		.and_then(|j| j.as_array())
		.into_iter()
		.flatten()
		.filter_map(|p| p.as_str());
	for p in jpath {
		// `join` keeps absolute paths as they are.
		out.push(roots.first().map_or_else(|| p.into(), |r| r.join(p)));
	}
	out
}

struct Server<'a> {
	connection: &'a Connection,
	docs: HashMap<Uri, Document>,
	search: Vec<PathBuf>,
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
			DocumentLinkRequest::METHOD => json(self.document_links(&params(&req)?)),
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
		let diagnostics = diagnostics(&doc, |path| self.resolve_import(&uri, path));
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

	/// The file an import path in the document at `uri` refers to, if it exists.
	fn resolve_import(&self, uri: &Uri, path: &str) -> Option<PathBuf> {
		let importer = uri::to_path(uri);
		imports::resolve(importer.as_deref(), path, &self.search, Path::exists)
	}

	fn document_links(&self, params: &DocumentLinkParams) -> Option<Vec<DocumentLink>> {
		let uri = &params.text_document.uri;
		let doc = self.docs.get(uri)?;
		Some(
			imports::imports(&doc.root)
				.into_iter()
				.filter_map(|i| {
					let target = uri::from_path(&self.resolve_import(uri, &i.path)?)?;
					Some(DocumentLink {
						range: doc.range(i.range),
						target: Some(target),
						tooltip: None,
						data: None,
					})
				})
				.collect(),
		)
	}

	fn definition(&self, params: &GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
		let pos = &params.text_document_position_params;
		let doc = self.docs.get(&pos.text_document.uri)?;
		if let Some(import) = doc.import_at(pos.position) {
			let target = uri::from_path(&self.resolve_import(&pos.text_document.uri, &import.path)?)?;
			let start = Position::new(0, 0);
			return Some(GotoDefinitionResponse::Scalar(Location::new(
				target,
				lsp_types::Range::new(start, start),
			)));
		}
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

fn diagnostics(doc: &Document, resolve_import: impl Fn(&str) -> Option<PathBuf>) -> Vec<Diagnostic> {
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
	// A warning, since the client's jpath may differ from what the real run uses (`jrsonnet -J`).
	let missing_imports = imports::imports(&doc.root)
		.into_iter()
		.filter(|i| resolve_import(&i.path).is_none())
		.map(|i| Diagnostic {
			range: doc.range(i.range),
			severity: Some(DiagnosticSeverity::WARNING),
			source: Some("jsonnet-lsp".to_owned()),
			message: format!("cannot find `{}` in the importing directory or search paths", i.path),
			..Default::default()
		});
	syntax.chain(unresolved).chain(missing_imports).collect()
}
