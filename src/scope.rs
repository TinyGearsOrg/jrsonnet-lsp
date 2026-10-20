//! Lexical name resolution over the rowan CST.
//!
//! Jsonnet scoping is purely lexical for variables, so none of this needs
//! evaluation. A declaration is identified by the range of its identifier token.

use jrsonnet_rowan_parser::{
	AstNode, SyntaxKind, SyntaxNode, SyntaxToken,
	nodes::{
		Bind, BindDestruct, CompSpec, Destruct, DestructRest, ExprArrayComp, ExprVar, MemberComp,
		Name, ObjBodyComp, ObjBodyMemberList, ParamsDesc,
	},
	rowan::{NodeOrToken, TextRange, TextSize},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decl {
	pub name: String,
	/// Range of the identifier token that introduces the name.
	pub range: TextRange,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Resolution {
	Local(Decl),
	/// The implicit `std` global.
	Std,
	Unresolved,
}

fn decl(name: &Name) -> Option<Decl> {
	let tok = name.ident_lit()?;
	Some(Decl {
		name: tok.text().to_owned(),
		range: tok.text_range(),
	})
}

/// `...rest` holds a bare identifier token, not the `Name` node the grammar describes.
fn rest_decl(rest: &DestructRest) -> Option<Decl> {
	let tok = rest
		.syntax()
		.children_with_tokens()
		.find_map(|e| e.into_token().filter(|t| t.kind() == SyntaxKind::IDENT))?;
	Some(Decl {
		name: tok.text().to_owned(),
		range: tok.text_range(),
	})
}

fn destruct_decls(d: &Destruct, out: &mut Vec<Decl>) {
	match d {
		Destruct::DestructFull(f) => out.extend(f.name().as_ref().and_then(decl)),
		Destruct::DestructSkip(_) => {}
		Destruct::DestructArray(a) => {
			// The parser puts elements directly under the array node, without the
			// `DestructArrayElement` wrapper the grammar describes, so the typed
			// `destruct_array_parts()` finds nothing. Look at the children instead.
			for child in a.syntax().children() {
				if let Some(d) = Destruct::cast(child.clone()) {
					destruct_decls(&d, out);
				} else if let Some(r) = DestructRest::cast(child) {
					out.extend(rest_decl(&r));
				}
			}
		}
		Destruct::DestructObject(o) => {
			for field in o.destruct_object_fields() {
				match field.destruct() {
					// `{a: inner}` binds only what `inner` binds.
					Some(d) => destruct_decls(&d, out),
					// `{a}` binds `a` itself.
					None => out.extend(field.field().as_ref().and_then(decl)),
				}
			}
			if let Some(r) = o.destruct_rest() {
				out.extend(rest_decl(&r));
			}
		}
	}
}

fn bind_decls(b: &Bind, out: &mut Vec<Decl>) {
	match b {
		Bind::BindDestruct(b) => {
			if let Some(d) = b.into() {
				destruct_decls(&d, out);
			}
		}
		Bind::BindFunction(b) => out.extend(b.name().as_ref().and_then(decl)),
	}
}

fn param_decls(p: &ParamsDesc, out: &mut Vec<Decl>) {
	for param in p.params() {
		if let Some(d) = param.destruct() {
			destruct_decls(&d, out);
		}
	}
}

/// Whether `spec` is in scope for a reference at `at`: the head and body of a
/// comprehension see every `for`, while a spec only sees the ones before it.
fn spec_visible(spec: &SyntaxNode, all: &[SyntaxNode], at: Option<TextRange>) -> bool {
	let Some(at) = at else { return true };
	spec.text_range().end() <= at.start() || !all.iter().any(|s| s.text_range().contains_range(at))
}

fn spec_decls(specs: impl Iterator<Item = CompSpec>, at: Option<TextRange>, out: &mut Vec<Decl>) {
	let specs: Vec<_> = specs.collect();
	let nodes: Vec<_> = specs.iter().map(|s| s.syntax().clone()).collect();
	for spec in &specs {
		if !spec_visible(spec.syntax(), &nodes, at) {
			continue;
		}
		match spec {
			CompSpec::ForSpec(f) => {
				if let Some(d) = f.bind() {
					destruct_decls(&d, out);
				}
			}
			CompSpec::ForObjSpec(f) => {
				out.extend(f.key().as_ref().and_then(decl));
				if let Some(d) = f.value() {
					destruct_decls(&d, out);
				}
			}
			CompSpec::IfSpec(_) => {}
		}
	}
}

/// Names that `node` brings into scope, in shadowing order (innermost last).
/// With `at`, only those visible to a reference at that range.
fn introduced(node: &SyntaxNode, at: Option<TextRange>) -> Vec<Decl> {
	let mut out = vec![];
	match node.kind() {
		SyntaxKind::EXPR => {
			for stmt in node
				.children()
				.filter(|c| c.kind() == SyntaxKind::STMT_LOCAL)
			{
				let r = stmt.text_range();
				// Binds see themselves (recursion) and everything after them.
				if at.is_some_and(|at| r.end() > at.start() && !r.contains_range(at)) {
					continue;
				}
				if let Some(local) = jrsonnet_rowan_parser::nodes::StmtLocal::cast(stmt) {
					for b in local.binds() {
						bind_decls(&b, &mut out);
					}
				}
			}
		}
		SyntaxKind::EXPR_FUNCTION | SyntaxKind::BIND_FUNCTION | SyntaxKind::MEMBER_FIELD_METHOD => {
			for p in node.children().filter_map(ParamsDesc::cast) {
				param_decls(&p, &mut out);
			}
		}
		SyntaxKind::EXPR_ARRAY_COMP => {
			if let Some(c) = ExprArrayComp::cast(node.clone()) {
				spec_decls(c.comp_specs(), at, &mut out);
			}
		}
		SyntaxKind::OBJ_BODY_MEMBER_LIST => {
			if let Some(b) = ObjBodyMemberList::cast(node.clone()) {
				for m in b.members() {
					if let jrsonnet_rowan_parser::nodes::Member::MemberBindStmt(l) = m
						&& let Some(b) = l.obj_local().and_then(|l| l.bind())
					{
						bind_decls(&b, &mut out);
					}
				}
			}
		}
		SyntaxKind::OBJ_BODY_COMP => {
			if let Some(b) = ObjBodyComp::cast(node.clone()) {
				for m in b.member_comps() {
					if let MemberComp::MemberBindStmt(l) = m
						&& let Some(b) = l.obj_local().and_then(|l| l.bind())
					{
						bind_decls(&b, &mut out);
					}
				}
				spec_decls(b.comp_specs(), at, &mut out);
			}
		}
		_ => {}
	}
	out
}

pub fn resolve(var: &ExprVar) -> Option<Resolution> {
	let name = var.name()?;
	let tok = name.ident_lit()?;
	let at = tok.text_range();
	for anc in var.syntax().ancestors() {
		if let Some(d) = introduced(&anc, Some(at))
			.into_iter()
			.rev()
			.find(|d| d.name == tok.text())
		{
			return Some(Resolution::Local(d));
		}
	}
	Some(if tok.text() == "std" {
		Resolution::Std
	} else {
		Resolution::Unresolved
	})
}

/// All declarations in the file.
pub fn declarations(root: &SyntaxNode) -> Vec<Decl> {
	root.descendants()
		.flat_map(|n| introduced(&n, None))
		.collect()
}

/// All uses of names in the file, with what each resolves to.
pub fn uses(root: &SyntaxNode) -> impl Iterator<Item = (Decl, Resolution)> + '_ {
	root.descendants()
		.filter_map(ExprVar::cast)
		.filter_map(|v| {
			let name = decl(&v.name()?)?;
			Some((name, resolve(&v)?))
		})
}

/// The declaration the identifier under the cursor refers to or declares.
pub fn decl_at(root: &SyntaxNode, tok: &SyntaxToken) -> Option<Resolution> {
	if tok.kind() != SyntaxKind::IDENT {
		return None;
	}
	let name = tok.parent()?;
	if let Some(var) = name.parent().and_then(ExprVar::cast) {
		return resolve(&var);
	}
	let range = tok.text_range();
	declarations(root)
		.into_iter()
		.find(|d| d.range == range)
		.map(Resolution::Local)
}

/// Ranges of every use of `target`, not including its declaration.
pub fn references(root: &SyntaxNode, target: &Decl) -> Vec<TextRange> {
	uses(root)
		.filter(|(_, r)| matches!(r, Resolution::Local(d) if d == target))
		.map(|(n, _)| n.range)
		.collect()
}

/// The expression a plain `local x = <expr>` name is bound to. Function binds, parameters,
/// comprehension variables and names inside a destructuring pattern have no single value.
pub fn decl_value(root: &SyntaxNode, decl: &Decl) -> Option<SyntaxNode> {
	let tok = root
		.token_at_offset(decl.range.start())
		.find(|t| t.text_range() == decl.range)?;
	let full = tok.parent()?.parent()?;
	if full.kind() != SyntaxKind::DESTRUCT_FULL {
		return None;
	}
	let bind = BindDestruct::cast(full.parent()?)?;
	if BindDestruct::into(&bind)?.syntax() != &full {
		return None;
	}
	Some(bind.value()?.syntax().clone())
}

/// Names usable at `offset`, innermost declarations first, each name once.
pub fn visible_at(root: &SyntaxNode, offset: TextSize) -> Vec<Decl> {
	let at = TextRange::empty(offset);
	let start = match root.covering_element(at) {
		NodeOrToken::Node(n) => Some(n),
		NodeOrToken::Token(t) => t.parent(),
	};
	let mut out: Vec<Decl> = vec![];
	for anc in start.into_iter().flat_map(|n| n.ancestors()) {
		for d in introduced(&anc, Some(at)).into_iter().rev() {
			if !out.iter().any(|o| o.name == d.name) {
				out.push(d);
			}
		}
	}
	out
}

/// What introduced `decl`: the enclosing bind, parameter or `for` spec, with a short label.
pub fn decl_site(root: &SyntaxNode, decl: &Decl) -> Option<(SyntaxNode, &'static str)> {
	let tok = root
		.token_at_offset(decl.range.start())
		.find(|t| t.text_range() == decl.range)?;
	tok.parent_ancestors().find_map(|n| {
		let label = match n.kind() {
			SyntaxKind::BIND_DESTRUCT | SyntaxKind::BIND_FUNCTION => "local ",
			SyntaxKind::PARAM => "(parameter) ",
			SyntaxKind::FOR_SPEC | SyntaxKind::FOR_OBJ_SPEC => "(loop variable) ",
			_ => return None,
		};
		Some((n, label))
	})
}

pub fn is_valid_ident(s: &str) -> bool {
	const KEYWORDS: &[&str] = &[
		"assert",
		"else",
		"error",
		"false",
		"for",
		"function",
		"if",
		"import",
		"importstr",
		"importbin",
		"in",
		"local",
		"null",
		"tailstrict",
		"then",
		"self",
		"super",
		"true",
	];
	let mut chars = s.chars();
	chars
		.next()
		.is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
		&& chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
		&& !KEYWORDS.contains(&s)
}

#[cfg(test)]
mod tests {
	use jrsonnet_rowan_parser::parse;

	use super::*;

	/// `«x»` marks the declaration, `‹x›` a use that must resolve to it.
	fn check(src: &str) {
		let (mut clean, mut decl_at_, mut use_at) = (String::new(), None, None);
		for c in src.chars() {
			match c {
				'«' => decl_at_ = Some(clean.len()),
				'‹' => use_at = Some(clean.len()),
				'»' | '›' => {}
				c => clean.push(c),
			}
		}
		let (root, errors) = parse(&clean);
		assert!(errors.is_empty(), "{errors:?} in {clean}");
		let root = root.syntax().clone();
		let tok = root
			.token_at_offset((use_at.unwrap() as u32).into())
			.find(|t| t.kind() == SyntaxKind::IDENT)
			.unwrap();
		let Some(Resolution::Local(got)) = decl_at(&root, &tok) else {
			panic!("`{}` didn't resolve to a local in {clean}", tok.text())
		};
		assert_eq!(
			usize::from(got.range.start()),
			decl_at_.unwrap(),
			"in {clean}"
		);
	}

	fn unresolved(src: &str) -> Vec<String> {
		let (root, errors) = parse(src);
		assert!(errors.is_empty(), "{errors:?}");
		uses(root.syntax())
			.filter(|(_, r)| *r == Resolution::Unresolved)
			.map(|(n, _)| n.name)
			.collect()
	}

	#[test]
	fn locals() {
		check("local «a» = 1; ‹a›");
		check("local «a» = 1, b = ‹a›; b");
		check("local «f»(x) = ‹f›(x); f(1)");
		check("local a = 1; local «a» = 2; ‹a›");
	}

	#[test]
	fn functions_and_destructuring() {
		check("function(«x», y = ‹x›) y");
		check("local [«a», b] = [1, 2]; ‹a›");
		check("local [a, ...«r»] = [1, 2]; ‹r›");
		check("local {«a», b: c} = {}; ‹a›");
		check("local {a: «c»} = {}; ‹c›");
		check("local {a, ...«r»} = {}; ‹r›");
	}

	#[test]
	fn objects_and_comprehensions() {
		check("{ local «a» = 1, b: ‹a› }");
		check("[‹x› for «x» in [1]]");
		check("[1 for «x» in [1] for y in ‹x›]");
		check("{ [‹k›]: 1 for «k» in [1] }");
	}

	#[test]
	fn unresolved_names() {
		assert_eq!(unresolved("local a = 1; b"), ["b"]);
		// Binds are not visible before their statement, and not outside their scope.
		assert_eq!(unresolved("local a = b; local b = 1; a"), ["b"]);
		assert_eq!(unresolved("[x for x in [1]] + x"), ["x"]);
		assert!(unresolved("std.length([])").is_empty());
		assert!(unresolved("[1 for x in [1] for y in x]").is_empty());
		assert_eq!(unresolved("[1 for x in y for y in [1]]"), ["y"]);
	}

	#[test]
	fn idents() {
		assert!(is_valid_ident("foo_1"));
		assert!(!is_valid_ident("1a"));
		assert!(!is_valid_ident("local"));
		assert!(!is_valid_ident(""));
	}
}
