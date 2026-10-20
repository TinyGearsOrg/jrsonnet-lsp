//! Signature help: the parameter list of the function being called, with the active parameter.
//!
//! The callee is found statically: a `local` function, a field reached through
//! [`fields`], or a `std` function. Anything else (parameters, call results) has none.

use std::rc::Rc;

use jrsonnet_rowan_parser::{
	AstNode, SyntaxKind, SyntaxNode,
	nodes::{Destruct, ExprVar, Param},
	rowan::TextSize,
};

use crate::{
	document::Document,
	fields::{self, Files},
	scope::{self, Resolution},
	stdlib,
};

pub struct Signature {
	pub label: String,
	/// UTF-16 offsets of each parameter within `label`.
	pub params: Vec<(u32, u32)>,
	/// Plain name of each parameter, where it has one (patterns don't).
	pub names: Vec<Option<String>>,
}

/// The call whose argument list the cursor is inside: the nearest `(...)` that the offset is
/// after the `(` of and before the `)` of, if there is one.
pub fn args_at(doc: &Document, offset: TextSize) -> Option<SyntaxNode> {
	let tok = doc.root.token_at_offset(offset).left_biased()?;
	tok.parent_ancestors()
		.filter(|n| n.kind() == SyntaxKind::ARGS_DESC)
		.find(|args| {
			let close = args
				.children_with_tokens()
				.filter_map(|e| e.into_token())
				.find(|t| t.kind() == SyntaxKind::R_PAREN);
			args.text_range().start() < offset
				&& close.is_none_or(|c| offset <= c.text_range().start())
		})
}

pub fn signature(files: &dyn Files, doc: &Rc<Document>, args: &SyntaxNode) -> Option<Signature> {
	let apply = args.parent()?;
	if let Some(member) = std_member(&apply) {
		let params = member.params?.iter().map(|p| {
			// `name = default`: the name is what a named argument matches.
			let name = p.split(' ').next().map(str::to_owned);
			((*p).to_owned(), name)
		});
		return Some(build(&format!("std.{}", member.name), params));
	}

	let (name, params) = callee(files, doc, &apply)?;
	let params = params.children().filter_map(Param::cast).map(|p| {
		// Defaults can span lines; a signature is one line.
		let text = p.syntax().text().to_string();
		let name = match p.destruct() {
			Some(Destruct::DestructFull(d)) => d
				.name()
				.and_then(|n| n.ident_lit())
				.map(|t| t.text().to_owned()),
			_ => None,
		};
		(text.split_whitespace().collect::<Vec<_>>().join(" "), name)
	});
	Some(build(&name, params))
}

/// `name(p1, p2)` from each parameter's text and plain name.
fn build(name: &str, params: impl Iterator<Item = (String, Option<String>)>) -> Signature {
	let mut label = format!("{name}(");
	let mut offsets = vec![];
	let mut names = vec![];
	for (i, (text, param_name)) in params.enumerate() {
		if i > 0 {
			label.push_str(", ");
		}
		let start = label.encode_utf16().count() as u32;
		label.push_str(&text);
		offsets.push((start, label.encode_utf16().count() as u32));
		names.push(param_name);
	}
	label.push(')');
	Signature {
		label,
		params: offsets,
		names,
	}
}

/// The `std` function called by `apply`, for a plain `std.name(...)`.
fn std_member(apply: &SyntaxNode) -> Option<&'static stdlib::Member> {
	let expr = apply.parent()?;
	let parts: Vec<_> = expr
		.children()
		.filter(|c| !matches!(c.kind(), SyntaxKind::STMT_LOCAL | SyntaxKind::STMT_ASSERT))
		.collect();
	let [base, index, call, ..] = parts.as_slice() else {
		return None;
	};
	if call != apply || index.kind() != SyntaxKind::SUFFIX_INDEX {
		return None;
	}
	let var = ExprVar::cast(base.clone())?;
	if scope::resolve(&var)? != Resolution::Std {
		return None;
	}
	let name = index.children().find(|c| c.kind() == SyntaxKind::NAME)?;
	stdlib::find(&name.text().to_string())
}

/// Which parameter the cursor is on: the one named by a `name = value` argument, otherwise
/// the position counted by commas.
pub fn active_param(args: &SyntaxNode, offset: TextSize, sig: &Signature) -> Option<u32> {
	let commas = args
		.children_with_tokens()
		.filter_map(|e| e.into_token())
		.filter(|t| t.kind() == SyntaxKind::COMMA && t.text_range().end() <= offset)
		.count();
	let named = args
		.children()
		.filter(|a| a.kind() == SyntaxKind::ARG && a.text_range().contains_inclusive(offset))
		.find_map(|a| {
			let name = a.children().find(|c| c.kind() == SyntaxKind::NAME)?;
			let name = name.text().to_string();
			sig.names
				.iter()
				.position(|n| n.as_deref() == Some(name.as_str()))
		});
	let active = named.unwrap_or(commas);
	(active < sig.params.len()).then_some(active as u32)
}

/// Name and `PARAMS_DESC` of the function that the call `apply` (a `SUFFIX_APPLY`) calls.
fn callee(
	files: &dyn Files,
	doc: &Rc<Document>,
	apply: &SyntaxNode,
) -> Option<(String, SyntaxNode)> {
	let expr = apply.parent()?;
	let parts: Vec<_> = expr
		.children()
		.filter(|c| !matches!(c.kind(), SyntaxKind::STMT_LOCAL | SyntaxKind::STMT_ASSERT))
		.collect();
	let i = parts.iter().position(|p| p == apply)?;
	let prev = parts.get(i.checked_sub(1)?)?;

	match prev.kind() {
		SyntaxKind::EXPR_VAR if i == 1 => local_function(files, doc, &ExprVar::cast(prev.clone())?),
		SyntaxKind::SUFFIX_INDEX => {
			let name = prev
				.children()
				.find(|c| c.kind() == SyntaxKind::NAME)?
				.text()
				.to_string();
			let field = fields::before_suffix(files, doc, prev)?
				.into_iter()
				.find(|f| f.name == name)?;
			let params = field_params(&field)?;
			Some((name, params))
		}
		_ => None,
	}
}

fn local_function(
	files: &dyn Files,
	doc: &Rc<Document>,
	var: &ExprVar,
) -> Option<(String, SyntaxNode)> {
	let Resolution::Local(decl) = scope::resolve(var)? else {
		return None;
	};
	let (site, _) = scope::decl_site(&doc.root, &decl)?;
	let params = match site.kind() {
		SyntaxKind::BIND_FUNCTION => params_desc(&site)?,
		SyntaxKind::BIND_DESTRUCT => {
			let (value, path) = scope::decl_source(&doc.root, &decl)?;
			if path.is_empty() {
				params_of_value(&value)?
			} else {
				// `local {greet} = lib;`: the function is a field of the destructured object.
				field_params(&fields::destructured_field(files, doc, &decl)?)?
			}
		}
		_ => return None,
	};
	Some((decl.name, params))
}

/// The parameters of a field that is a method or has a function literal as its value.
fn field_params(field: &fields::Field) -> Option<SyntaxNode> {
	match field.member()? {
		m if m.kind() == SyntaxKind::MEMBER_FIELD_METHOD => params_desc(&m),
		_ => params_of_value(field.value.as_ref()?),
	}
}

fn params_desc(node: &SyntaxNode) -> Option<SyntaxNode> {
	node.children()
		.find(|c| c.kind() == SyntaxKind::PARAMS_DESC)
}

/// The parameters of an expression that is a function literal.
fn params_of_value(expr: &SyntaxNode) -> Option<SyntaxNode> {
	let base = expr
		.children()
		.find(|c| !matches!(c.kind(), SyntaxKind::STMT_LOCAL | SyntaxKind::STMT_ASSERT))?;
	(base.kind() == SyntaxKind::EXPR_FUNCTION).then(|| params_desc(&base))?
}
