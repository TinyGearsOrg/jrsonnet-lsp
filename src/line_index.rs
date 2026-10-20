//! Conversion between byte offsets and LSP positions (line + UTF-16 code unit).

use lsp_types::{Position, Range};

pub struct LineIndex {
	/// Byte offset of the start of each line.
	starts: Vec<usize>,
}

impl LineIndex {
	pub fn new(text: &str) -> Self {
		let mut starts = vec![0];
		starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
		Self { starts }
	}

	pub fn position(&self, text: &str, offset: usize) -> Position {
		let offset = offset.min(text.len());
		let line = self.starts.partition_point(|&s| s <= offset) - 1;
		let col = text[self.starts[line]..offset]
			.chars()
			.map(char::len_utf16)
			.sum::<usize>();
		Position::new(line as u32, col as u32)
	}

	pub fn range(&self, text: &str, start: usize, end: usize) -> Range {
		Range::new(self.position(text, start), self.position(text, end))
	}

	/// Range covering the whole document.
	pub fn full_range(&self, text: &str) -> Range {
		self.range(text, 0, text.len())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn utf16_columns() {
		let text = "a\n😀b\n";
		let idx = LineIndex::new(text);
		// '😀' is 4 bytes, 2 UTF-16 units.
		assert_eq!(idx.position(text, 2), Position::new(1, 0));
		assert_eq!(idx.position(text, 6), Position::new(1, 2));
		assert_eq!(idx.position(text, text.len()), Position::new(2, 0));
	}
}
