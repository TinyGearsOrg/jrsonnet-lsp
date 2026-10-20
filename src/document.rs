use jrsonnet_rowan_parser::{
	AstNode, LocatedSyntaxError, SyntaxKind, SyntaxNode, SyntaxToken, parse, rowan::TextRange,
};
use lsp_types::{Position, TextDocumentContentChangeEvent, Uri};

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

/// Applies `didChange` events in order; each range refers to the text as the previous events
/// left it. A change without a range replaces the whole text.
pub fn apply_changes(mut text: String, changes: Vec<TextDocumentContentChangeEvent>) -> String {
	for change in changes {
		let Some(range) = change.range else {
			text = change.text;
			continue;
		};
		let index = LineIndex::new(&text);
		// Positions past the end clamp to it.
		let start = index.offset(&text, range.start).unwrap_or(text.len());
		let end = index.offset(&text, range.end).unwrap_or(text.len());
		text.replace_range(start.min(end)..end, &change.text);
	}
	text
}

#[cfg(test)]
mod tests {
	use lsp_types::Range;

	use super::*;

	fn edit(l1: u32, c1: u32, l2: u32, c2: u32, text: &str) -> TextDocumentContentChangeEvent {
		TextDocumentContentChangeEvent {
			range: Some(Range::new(Position::new(l1, c1), Position::new(l2, c2))),
			range_length: None,
			text: text.to_owned(),
		}
	}

	#[test]
	fn replaces_inserts_and_deletes() {
		assert_eq!(
			apply_changes("{a: 1}".to_owned(), vec![edit(0, 4, 0, 5, "xyz")]),
			"{a: xyz}"
		);
		assert_eq!(
			apply_changes("ab".to_owned(), vec![edit(0, 1, 0, 1, "-")]),
			"a-b"
		);
		assert_eq!(
			apply_changes("a\nb\nc".to_owned(), vec![edit(0, 1, 2, 0, "")]),
			"ac"
		);
	}

	#[test]
	fn columns_are_utf16() {
		// In `"é😀" + x` the quote, `é`, `😀` (two units, four bytes), quote, space and `+` are
		// at columns 0, 1, 2..4, 4, 5 and 6, so columns 5 to 8 are ` + `.
		let t = apply_changes("\"é😀\" + x".to_owned(), vec![edit(0, 5, 0, 8, "")]);
		assert_eq!(t, "\"é😀\"x");
	}

	#[test]
	fn later_changes_see_earlier_ones() {
		// `xyz` -> `abcyz`, then columns 3..4 are the `y`.
		let changes = vec![edit(0, 0, 0, 1, "abc"), edit(0, 3, 0, 4, "Z")];
		assert_eq!(apply_changes("xyz".to_owned(), changes), "abcZz");
	}

	#[test]
	fn whole_text_and_clamping() {
		let changes = vec![
			edit(0, 0, 0, 1, "q"),
			TextDocumentContentChangeEvent {
				range: None,
				range_length: None,
				text: "new".to_owned(),
			},
			// A range past the end of the text clamps to the end.
			edit(5, 0, 9, 9, "!"),
		];
		assert_eq!(apply_changes("old".to_owned(), changes), "new!");
	}
}
