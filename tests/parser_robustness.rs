//! The parser must never panic, whatever the editor hands it mid-edit.

use jrsonnet_rowan_parser::parse;

/// Each of these once panicked: a field name with no `:` (or one error recovery ate the last token
/// of) made `visibility` bump past EOF, and `function` / `import` without their operand tripped asserts.
#[test]
fn incomplete_objects_do_not_panic() {
	for src in [
		"{",
		"local base = {",
		"{ a",
		"{ a: 1, +\n}",
		"{ a: [], +\n}",
		"x { a: [1], + }",
		"local f = function",
		"function",
		"local o = import",
		"import",
	] {
		let (_, errors) = parse(src);
		assert!(!errors.is_empty(), "{src:?} should be a syntax error");
	}
}

/// Every prefix of a document is what the server sees while the user types it.
#[test]
fn every_prefix_parses() {
	let src = "local orgs = import 'x.libsonnet';\n\
		orgs.newOrg('a') {\n  settings+: { b: self.c, d: \"e\", f+: { g: false } },\n  \
		_repos+:: [ orgs.newRepo('r') { t+: [ \"x\" ], w: { y: null } }, ], +\n}\n";
	for i in (0..=src.len()).filter(|&i| src.is_char_boundary(i)) {
		parse(&src[..i]);
	}
}
