//! `import`, `importstr` and `importbin` paths and how they resolve to files.

use std::path::{Path, PathBuf};

use jrsonnet_rowan_parser::{
	AstNode, AstToken, SyntaxKind, SyntaxNode,
	nodes::{ExprImport, Text},
	rowan::TextRange,
};

#[derive(Debug, PartialEq, Eq)]
pub struct Import {
	pub path: String,
	/// Range of the string literal, quotes included.
	pub range: TextRange,
}

/// Every import in the file whose path is a plain (non-block) string literal.
pub fn imports(root: &SyntaxNode) -> Vec<Import> {
	root.descendants()
		.filter_map(ExprImport::cast)
		.filter_map(|i| {
			let text: Text = i.text()?;
			let tok = text.syntax();
			Some(Import {
				path: string_value(tok.kind(), tok.text())?,
				range: tok.text_range(),
			})
		})
		.collect()
}

/// Value of a string literal. Block strings and malformed literals give `None`.
pub fn string_value(kind: SyntaxKind, lit: &str) -> Option<String> {
	match kind {
		SyntaxKind::STRING_DOUBLE | SyntaxKind::STRING_SINGLE => {
			let inner = lit.get(1..lit.len().checked_sub(1)?)?;
			let mut out = String::new();
			let mut chars = inner.chars();
			while let Some(c) = chars.next() {
				if c != '\\' {
					out.push(c);
					continue;
				}
				match chars.next()? {
					'n' => out.push('\n'),
					't' => out.push('\t'),
					'r' => out.push('\r'),
					c @ ('\\' | '"' | '\'' | '/') => out.push(c),
					// `\uXXXX` and friends: not worth guessing at in a path.
					_ => return None,
				}
			}
			Some(out)
		}
		// `@"..."`: no escapes, a doubled quote stands for one quote.
		SyntaxKind::STRING_DOUBLE_VERBATIM | SyntaxKind::STRING_SINGLE_VERBATIM => {
			let inner = lit.get(2..lit.len().checked_sub(1)?)?;
			let quote = lit.chars().nth(1)?;
			Some(inner.replace(&format!("{quote}{quote}"), &quote.to_string()))
		}
		_ => None,
	}
}

/// Where an import would be found: relative to the importing file first, then each search path.
pub fn resolve(
	importer: Option<&Path>,
	path: &str,
	search: &[PathBuf],
	exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
	let rel = Path::new(path);
	if rel.is_absolute() {
		return exists(rel).then(|| rel.to_owned());
	}
	importer
		.and_then(Path::parent)
		.into_iter()
		.chain(search.iter().map(PathBuf::as_path))
		.map(|dir| dir.join(rel))
		.find(|p| exists(p))
}

#[cfg(test)]
mod tests {
	use jrsonnet_rowan_parser::parse;

	use super::*;

	fn paths(src: &str) -> Vec<String> {
		let (root, _) = parse(src);
		imports(root.syntax()).into_iter().map(|i| i.path).collect()
	}

	#[test]
	fn collects_all_import_kinds() {
		assert_eq!(
			paths(
				r#"[import "a.libsonnet", importstr 'b.txt', importbin "c.bin", import @"d""e"]"#
			),
			["a.libsonnet", "b.txt", "c.bin", "d\"e"]
		);
	}

	#[test]
	fn skips_unsupported_strings() {
		assert_eq!(paths(r#"import "a\u0041""#), Vec::<String>::new());
	}

	#[test]
	fn range_covers_literal() {
		let (root, _) = parse(r#"import "a""#);
		let got = imports(root.syntax());
		assert_eq!(got[0].range, TextRange::new(7.into(), 10.into()));
	}

	#[test]
	fn resolution_order() {
		let have = |ps: &'static [&str]| move |p: &Path| ps.contains(&p.to_str().unwrap());
		let search = [PathBuf::from("/ws/vendor")];
		let importer = Some(Path::new("/ws/src/main.jsonnet"));
		// Importer's directory wins over search paths.
		assert_eq!(
			resolve(
				importer,
				"l.libsonnet",
				&search,
				have(&["/ws/src/l.libsonnet", "/ws/vendor/l.libsonnet"])
			),
			Some("/ws/src/l.libsonnet".into())
		);
		assert_eq!(
			resolve(
				importer,
				"l.libsonnet",
				&search,
				have(&["/ws/vendor/l.libsonnet"])
			),
			Some("/ws/vendor/l.libsonnet".into())
		);
		assert_eq!(resolve(importer, "l.libsonnet", &search, have(&[])), None);
		assert_eq!(
			resolve(None, "/abs/x", &search, have(&["/abs/x"])),
			Some("/abs/x".into())
		);
	}
}
