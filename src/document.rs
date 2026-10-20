use jrsonnet_rowan_parser::{
	AstNode, LocatedSyntaxError, SyntaxKind, SyntaxNode, SyntaxToken, parse, rowan::TextRange,
};
use lsp_types::{Position, Uri};

use crate::{
	imports,
	line_index::LineIndex,
	scope::{self, Decl, Resolution},
};

/// A parsed source file, either open in the editor or read from disk for an import.
pub struct Document {
	pub uri: Uri,
	pub text: String,
	pub index: LineIndex,
	pub root: SyntaxNode,
	pub errors: Vec<LocatedSyntaxError>,
}

impl Document {
	pub fn new(uri: Uri, text: String) -> Self {
		let index = LineIndex::new(&text);
		let (root, errors) = parse(&text);
		Self {
			uri,
			text,
			index,
			root: root.syntax().clone(),
			errors,
		}
	}

	pub fn range(&self, r: TextRange) -> lsp_types::Range {
		self.index
			.range(&self.text, usize::from(r.start()), usize::from(r.end()))
	}

	pub fn offset(&self, pos: Position) -> Option<u32> {
		u32::try_from(self.index.offset(&self.text, pos)?).ok()
	}

	/// The identifier token under the cursor, if any.
	pub fn ident_at(&self, pos: Position) -> Option<SyntaxToken> {
		self.root
			.token_at_offset(self.offset(pos)?.into())
			.find(|t| t.kind() == SyntaxKind::IDENT)
	}

	/// The import whose path string is under the cursor.
	pub fn import_at(&self, pos: Position) -> Option<imports::Import> {
		let offset = self.offset(pos)?;
		imports::imports(&self.root)
			.into_iter()
			.find(|i| i.range.contains_inclusive(offset.into()))
	}

	/// The local declaration the identifier at `pos` refers to or declares.
	pub fn decl_at(&self, pos: Position) -> Option<Decl> {
		match scope::decl_at(&self.root, &self.ident_at(pos)?)? {
			Resolution::Local(d) => Some(d),
			Resolution::Std | Resolution::Unresolved => None,
		}
	}
}
