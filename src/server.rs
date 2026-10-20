use std::collections::HashMap;

use anyhow::Result;
use jrsonnet_formatter::{FormatOptions, format};
use jrsonnet_rowan_parser::parse;
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{
	Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
	DidOpenTextDocumentParams, DocumentFormattingParams, InitializeParams, OneOf,
	PublishDiagnosticsParams, ServerCapabilities, TextDocumentSyncCapability,
	TextDocumentSyncKind, TextEdit, Uri,
	notification::{
		DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
		PublishDiagnostics,
	},
	request::{Formatting, Request as _},
};

use crate::line_index::LineIndex;

struct Document {
	text: String,
	index: LineIndex,
}

impl Document {
	fn new(text: String) -> Self {
		let index = LineIndex::new(&text);
		Self { text, index }
	}
}

pub fn run() -> Result<()> {
	let (connection, io_threads) = Connection::stdio();
	let capabilities = serde_json::to_value(ServerCapabilities {
		text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
		document_formatting_provider: Some(OneOf::Left(true)),
		..Default::default()
	})?;
	let init = connection.initialize(capabilities)?;
	let _params: InitializeParams = serde_json::from_value(init)?;

	Server {
		connection: &connection,
		docs: HashMap::new(),
	}
	.main_loop()?;
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
		let resp = match req.method.as_str() {
			Formatting::METHOD => {
				let params: DocumentFormattingParams = serde_json::from_value(req.params)?;
				Response::new_ok(req.id, self.formatting(&params))
			}
			_ => Response::new_err(
				req.id,
				lsp_server::ErrorCode::MethodNotFound as i32,
				format!("unsupported request {}", req.method),
			),
		};
		self.connection.sender.send(resp.into())?;
		Ok(())
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
}

fn diagnostics(doc: &Document) -> Vec<Diagnostic> {
	let (_, errors) = parse(&doc.text);
	errors
		.into_iter()
		.map(|e| {
			let start = usize::from(e.range.start());
			let end = usize::from(e.range.end());
			Diagnostic {
				range: doc.index.range(&doc.text, start, end),
				// TODO: SyntaxError isn't exported upstream, so `Hint`s can't be told apart.
				severity: Some(DiagnosticSeverity::ERROR),
				source: Some("jsonnet-lsp".to_owned()),
				message: e.error.to_string(),
				..Default::default()
			}
		})
		.collect()
}
