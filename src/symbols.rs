//! Document outline: top-level locals and object fields, nested objects as children.

use jrsonnet_rowan_parser::{
	AstNode, SyntaxKind, SyntaxNode,
	nodes::{Bind, ExprObject, FieldName, Member, ObjBody, StmtLocal},
	rowan::TextRange,
};
use lsp_types::{DocumentSymbol, SymbolKind};

use crate::{document::Document, imports};

pub fn symbols(doc: &Document) -> Vec<DocumentSymbol> {
	doc.root
		.children()
		.find(|c| c.kind() == SyntaxKind::EXPR)
		.map(|e| expr_symbols(doc, &e))
		.unwrap_or_default()
}

fn expr_symbols(doc: &Document, expr: &SyntaxNode) -> Vec<DocumentSymbol> {
	let mut out = vec![];
	let mut base = None;
	for child in expr.children() {
		match child.kind() {
			SyntaxKind::STMT_LOCAL => {
				if let Some(local) = StmtLocal::cast(child) {
					for bind in local.binds() {
						out.extend(bind_symbol(doc, &bind));
					}
				}
			}
			SyntaxKind::STMT_ASSERT => {}
			_ => {
				base.get_or_insert(child);
			}
		}
	}
	if let Some(obj) = base.and_then(ExprObject::cast) {
		out.extend(object_symbols(doc, &obj));
	}
	out
}

fn bind_symbol(doc: &Document, bind: &Bind) -> Option<DocumentSymbol> {
	let (name, kind) = match bind {
		Bind::BindFunction(f) => (f.name()?.ident_lit()?, SymbolKind::FUNCTION),
		// Destructuring patterns declare several names; only plain `local x = ...` is listed.
		Bind::BindDestruct(b) => match b.into()? {
			jrsonnet_rowan_parser::nodes::Destruct::DestructFull(d) => {
				(d.name()?.ident_lit()?, SymbolKind::VARIABLE)
			}
			_ => return None,
		},
	};
	Some(symbol(
		doc,
		name.text().to_owned(),
		kind,
		bind.syntax().text_range(),
		name.text_range(),
		vec![],
	))
}

fn object_symbols(doc: &Document, obj: &ExprObject) -> Vec<DocumentSymbol> {
	let Some(ObjBody::ObjBodyMemberList(body)) = obj.obj_body() else {
		return vec![];
	};
	let mut out = vec![];
	for member in body.members() {
		match member {
			Member::MemberBindStmt(l) => {
				out.extend(l.obj_local().and_then(|l| l.bind()).and_then(|b| bind_symbol(doc, &b)));
			}
			Member::MemberFieldNormal(f) => {
				let Some((name, sel)) = field_name(&f.field_name()) else { continue };
				let value = f.expr();
				let children = value
					.as_ref()
					.map(|e| expr_symbols(doc, e.syntax()))
					.unwrap_or_default();
				let kind = if value.is_some_and(|e| is_object(e.syntax())) {
					SymbolKind::OBJECT
				} else {
					SymbolKind::FIELD
				};
				out.push(symbol(doc, name, kind, f.syntax().text_range(), sel, children));
			}
			Member::MemberFieldMethod(f) => {
				let Some((name, sel)) = field_name(&f.field_name()) else { continue };
				out.push(symbol(doc, name, SymbolKind::METHOD, f.syntax().text_range(), sel, vec![]));
			}
			Member::MemberAssertStmt(_) => {}
		}
	}
	out
}

fn is_object(expr: &SyntaxNode) -> bool {
	expr.children()
		.find(|c| !matches!(c.kind(), SyntaxKind::STMT_LOCAL | SyntaxKind::STMT_ASSERT))
		.is_some_and(|c| c.kind() == SyntaxKind::EXPR_OBJECT)
}

/// Name and name range of a fixed field name; computed names have nothing to show.
fn field_name(name: &Option<FieldName>) -> Option<(String, TextRange)> {
	let Some(FieldName::FieldNameFixed(n)) = name else {
		return None;
	};
	if let Some(id) = n.id().and_then(|n| n.ident_lit()) {
		return Some((id.text().to_owned(), id.text_range()));
	}
	let text = jrsonnet_rowan_parser::AstToken::syntax(&n.text()?).clone();
	Some((imports::string_value(text.kind(), text.text())?, text.text_range()))
}

#[allow(deprecated)]
fn symbol(
	doc: &Document,
	name: String,
	kind: SymbolKind,
	range: TextRange,
	selection: TextRange,
	children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
	DocumentSymbol {
		name,
		detail: None,
		kind,
		tags: None,
		deprecated: None,
		range: doc.range(range),
		selection_range: doc.range(selection),
		children: (!children.is_empty()).then_some(children),
	}
}
