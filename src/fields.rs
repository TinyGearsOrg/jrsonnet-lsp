//! Best-effort static lookup of an object expression's fields, for go-to-definition and
//! completion after `.`.
//!
//! This follows only what can be known without evaluating: object literals, `+` and object
//! extension, parentheses, `local` names bound to such expressions, `import`s and chained
//! `.field` access, plus `self`, `super` and `$` inside object literals. Anything else (calls,
//! conditionals, comprehensions, `std`) gives `None` rather than a guess.

use std::rc::Rc;

use jrsonnet_rowan_parser::{
	AstNode, AstToken, SyntaxKind, SyntaxNode,
	nodes::{
		Expr, ExprBinary, ExprImport, ExprObjExtend, ExprObject, ExprParened, ExprVar,
		FieldName, Member, ObjBody, SuffixIndex,
	},
	nodes::ImportKindKind,
	rowan::TextRange,
};

use crate::{
	document::Document,
	imports,
	scope::{self, Resolution},
};

/// Locates other files. The server implements this over open buffers and the disk.
pub trait Files {
	fn import(&self, from: &Document, path: &str) -> Option<Rc<Document>>;
}

#[derive(Clone)]
pub struct Field {
	pub name: String,
	pub doc: Rc<Document>,
	/// The field's name in its declaration.
	pub name_range: TextRange,
	/// The field's value, when it is a plain expression (not a method).
	pub value: Option<SyntaxNode>,
}

pub type Fields = Vec<Field>;

/// Bounds the work on recursive definitions such as `local a = b, b = a`.
const MAX_DEPTH: u8 = 16;

/// Fields of the object `expr` evaluates to, applying its suffixes up to (not including)
/// `stop`, if given.
pub fn eval(
	files: &dyn Files,
	doc: &Rc<Document>,
	expr: &SyntaxNode,
	stop: Option<&SyntaxNode>,
	depth: u8,
) -> Option<Fields> {
	if depth > MAX_DEPTH {
		return None;
	}
	let mut parts = expr
		.children()
		.filter(|c| !matches!(c.kind(), SyntaxKind::STMT_LOCAL | SyntaxKind::STMT_ASSERT));
	let base = parts.next()?;
	let mut fields = base_fields(files, doc, &base, depth)?;
	for suffix in parts {
		if Some(&suffix) == stop {
			break;
		}
		let name = SuffixIndex::cast(suffix)?.index()?.ident_lit()?;
		let field = fields.into_iter().find(|f| f.name == name.text())?;
		fields = eval(files, &field.doc, field.value.as_ref()?, None, depth + 1)?;
	}
	Some(fields)
}

/// Fields visible after the `.` of `suffix`.
pub fn before_suffix(files: &dyn Files, doc: &Rc<Document>, suffix: &SyntaxNode) -> Option<Fields> {
	eval(files, doc, &suffix.parent()?, Some(suffix), 0)
}

fn base_fields(files: &dyn Files, doc: &Rc<Document>, node: &SyntaxNode, depth: u8) -> Option<Fields> {
	let sub = |e: Option<Expr>| eval(files, doc, e?.syntax(), None, depth + 1);
	match node.kind() {
		SyntaxKind::EXPR_OBJECT => object_fields(doc, &ExprObject::cast(node.clone())?),
		SyntaxKind::EXPR_PARENED => sub(ExprParened::cast(node.clone())?.expr()),
		SyntaxKind::EXPR_LITERAL => {
			let kw = node.first_token()?.kind();
			keyword_fields(files, doc, node, kw, depth)
		}
		SyntaxKind::EXPR_OBJ_EXTEND => {
			let ext = ExprObjExtend::cast(node.clone())?;
			Some(merge(sub(ext.lhs())?, sub(ext.rhs())?))
		}
		SyntaxKind::EXPR_BINARY => {
			let bin = ExprBinary::cast(node.clone())?;
			// The operator is a bare token child; only `+` combines objects.
			let is_plus = node
				.children_with_tokens()
				.any(|e| e.kind() == SyntaxKind::PLUS);
			if !is_plus {
				return None;
			}
			Some(merge(sub(bin.lhs())?, sub(bin.rhs())?))
		}
		SyntaxKind::EXPR_IMPORT => {
			let import = ExprImport::cast(node.clone())?;
			if import.import_kind()?.kind() != ImportKindKind::ImportKw {
				return None;
			}
			let text = import.text()?;
			let path = imports::string_value(text.syntax().kind(), text.syntax().text())?;
			let target = files.import(doc, &path)?;
			let root = target.root.children().find(|c| c.kind() == SyntaxKind::EXPR)?;
			eval(files, &target, &root, None, depth + 1)
		}
		SyntaxKind::EXPR_VAR => {
			let var = ExprVar::cast(node.clone())?;
			let Resolution::Local(decl) = scope::resolve(&var)? else {
				return None;
			};
			let value = scope::decl_value(&doc.root, &decl)?;
			eval(files, doc, &value, None, depth + 1)
		}
		_ => None,
	}
}

/// The left side of `lhs + {...}` / `lhs {...}` when the object literal `obj` is its right side.
/// That is what `super` refers to inside `obj`, and `self` also includes it.
fn super_expr(obj: &SyntaxNode) -> Option<SyntaxNode> {
	let wrapper = obj.parent().filter(|p| p.kind() == SyntaxKind::EXPR)?;
	// `{...}.x` or `local ...; {...}` are not the object itself.
	if wrapper.children().count() != 1 {
		return None;
	}
	let container = wrapper.parent()?;
	let combines = match container.kind() {
		SyntaxKind::EXPR_OBJ_EXTEND => true,
		SyntaxKind::EXPR_BINARY => container
			.children_with_tokens()
			.any(|e| e.kind() == SyntaxKind::PLUS),
		_ => false,
	};
	let mut exprs = container.children().filter(|c| c.kind() == SyntaxKind::EXPR);
	let (lhs, rhs) = (exprs.next()?, exprs.next()?);
	(combines && rhs == wrapper).then_some(lhs)
}

/// Fields of the object literal `obj` as seen by `self`: its own plus those it extends.
fn self_fields(files: &dyn Files, doc: &Rc<Document>, obj: &SyntaxNode, depth: u8) -> Option<Fields> {
	let own = object_fields(doc, &ExprObject::cast(obj.clone())?)?;
	let inherited = super_expr(obj)
		.and_then(|lhs| eval(files, doc, &lhs, None, depth + 1))
		.unwrap_or_default();
	Some(merge(inherited, own))
}

/// `self`, `super` and `$` in the object literals that enclose `node`.
fn keyword_fields(files: &dyn Files, doc: &Rc<Document>, node: &SyntaxNode, kw: SyntaxKind, depth: u8) -> Option<Fields> {
	let mut objects = node.ancestors().filter(|n| n.kind() == SyntaxKind::EXPR_OBJECT);
	match kw {
		SyntaxKind::SELF_KW => self_fields(files, doc, &objects.next()?, depth),
		SyntaxKind::SUPER_KW => eval(files, doc, &super_expr(&objects.next()?)?, None, depth + 1),
		// `$` is the outermost enclosing object.
		SyntaxKind::DOLLAR => self_fields(files, doc, &objects.last()?, depth),
		_ => None,
	}
}

fn object_fields(doc: &Rc<Document>, obj: &ExprObject) -> Option<Fields> {
	let ObjBody::ObjBodyMemberList(body) = obj.obj_body()? else {
		return None;
	};
	let mut out = vec![];
	for member in body.members() {
		let (name, value) = match member {
			Member::MemberFieldNormal(f) => (f.field_name(), f.expr().map(|e| e.syntax().clone())),
			Member::MemberFieldMethod(f) => (f.field_name(), None),
			_ => continue,
		};
		// Computed names (`[expr]: ...`) are unknown statically.
		let Some(FieldName::FieldNameFixed(name)) = name else {
			continue;
		};
		let (name, name_range) = if let Some(id) = name.id().and_then(|n| n.ident_lit()) {
			(id.text().to_owned(), id.text_range())
		} else if let Some(text) = name.text() {
			let tok = text.syntax();
			let Some(value) = imports::string_value(tok.kind(), tok.text()) else {
				continue;
			};
			(value, tok.text_range())
		} else {
			continue;
		};
		out = merge(
			out,
			vec![Field {
				name,
				doc: doc.clone(),
				name_range,
				value,
			}],
		);
	}
	Some(out)
}

/// `base + over`: fields of `over` replace same-named fields of `base`.
fn merge(mut base: Fields, over: Fields) -> Fields {
	for f in over {
		match base.iter_mut().find(|b| b.name == f.name) {
			Some(slot) => *slot = f,
			None => base.push(f),
		}
	}
	base
}

impl Field {
	/// The whole `name: value` member this field was declared by.
	pub fn member(&self) -> Option<SyntaxNode> {
		let tok = self
			.doc
			.root
			.token_at_offset(self.name_range.start())
			.find(|t| t.text_range() == self.name_range)?;
		tok.parent_ancestors().find(|n| {
			matches!(n.kind(), SyntaxKind::MEMBER_FIELD_NORMAL | SyntaxKind::MEMBER_FIELD_METHOD)
		})
	}
}
