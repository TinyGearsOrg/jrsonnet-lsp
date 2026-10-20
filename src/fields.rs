//! Best-effort static lookup of an object expression's fields, for go-to-definition and
//! completion after `.`.
//!
//! This follows only what can be known without evaluating: object and array literals, `+` and
//! object extension, parentheses, `local` names bound to such expressions (also through object
//! and array destructuring), `import`s, chained `.field` / `["field"]` / `[0]` access, plus
//! `self`, `super` and `$` inside object literals. Anything else (calls, conditionals,
//! comprehensions, `std`) gives `None` rather than a guess.

use std::rc::Rc;

use jrsonnet_rowan_parser::{
	AstNode, AstToken, SyntaxKind, SyntaxNode,
	nodes::ImportKindKind,
	nodes::{
		Expr, ExprBinary, ExprImport, ExprObjExtend, ExprObject, ExprParened, ExprVar, FieldName,
		Member, ObjBody, SuffixIndex,
	},
	rowan::TextRange,
};

use crate::{
	document::Document,
	imports,
	scope::{self, Resolution, Step},
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

/// An element of an array literal.
#[derive(Clone)]
pub struct Elem {
	pub doc: Rc<Document>,
	pub expr: SyntaxNode,
}

/// What an expression statically is: an object with known fields, or an array literal.
enum Shape {
	Object(Fields),
	Array(Vec<Elem>),
}

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
	match eval_shape(files, doc, expr, stop, depth)? {
		Shape::Object(fields) => Some(fields),
		Shape::Array(_) => None,
	}
}

fn eval_shape(
	files: &dyn Files,
	doc: &Rc<Document>,
	expr: &SyntaxNode,
	stop: Option<&SyntaxNode>,
	depth: u8,
) -> Option<Shape> {
	if depth > MAX_DEPTH {
		return None;
	}
	let mut parts = expr
		.children()
		.filter(|c| !matches!(c.kind(), SyntaxKind::STMT_LOCAL | SyntaxKind::STMT_ASSERT));
	let base = parts.next()?;
	let mut shape = base_shape(files, doc, &base, depth)?;
	for suffix in parts {
		if Some(&suffix) == stop {
			break;
		}
		shape = step(files, shape, &suffix_step(&suffix)?, depth)?;
	}
	Some(shape)
}

/// What a `.name`, `["name"]` or `[0]` suffix (also with `?.`) looks up.
fn suffix_step(suffix: &SyntaxNode) -> Option<Step> {
	match suffix.kind() {
		SyntaxKind::SUFFIX_INDEX => {
			let name = SuffixIndex::cast(suffix.clone())?.index()?.ident_lit()?;
			Some(Step::Field(name.text().to_owned()))
		}
		SyntaxKind::SUFFIX_INDEX_EXPR => {
			let index = suffix.children().find(|c| c.kind() == SyntaxKind::EXPR)?;
			literal_step(&index)
		}
		_ => None,
	}
}

/// `"name"` or `3` on its own as an index expression.
fn literal_step(expr: &SyntaxNode) -> Option<Step> {
	let mut parts = expr
		.children()
		.filter(|c| !matches!(c.kind(), SyntaxKind::STMT_LOCAL | SyntaxKind::STMT_ASSERT));
	let (base, None) = (parts.next()?, parts.next()) else {
		return None;
	};
	let tok = base.first_token()?;
	match base.kind() {
		SyntaxKind::EXPR_STRING => imports::string_value(tok.kind(), tok.text()).map(Step::Field),
		SyntaxKind::EXPR_NUMBER => tok.text().parse().ok().map(Step::Index),
		_ => None,
	}
}

/// Applies one lookup to a shape.
fn step(files: &dyn Files, shape: Shape, step: &Step, depth: u8) -> Option<Shape> {
	let (doc, value) = match (shape, step) {
		(Shape::Object(fields), Step::Field(name)) => {
			let field = fields.into_iter().find(|f| &f.name == name)?;
			(field.doc, field.value?)
		}
		(Shape::Array(elems), Step::Index(i)) => {
			let elem = elems.into_iter().nth(*i)?;
			(elem.doc, elem.expr)
		}
		_ => return None,
	};
	eval_shape(files, &doc, &value, None, depth + 1)
}

/// Fields visible after the `.` of `suffix`.
pub fn before_suffix(files: &dyn Files, doc: &Rc<Document>, suffix: &SyntaxNode) -> Option<Fields> {
	eval(files, doc, &suffix.parent()?, Some(suffix), 0)
}

/// The field that a `local` name stands for when it was bound by object destructuring, such as
/// `greet` in `local {greet} = lib;` or `x` in `local {a: {b: x}} = lib;`.
pub fn destructured_field(
	files: &dyn Files,
	doc: &Rc<Document>,
	decl: &scope::Decl,
) -> Option<Field> {
	let (value, mut path) = scope::decl_source(&doc.root, decl)?;
	let Some(Step::Field(name)) = path.pop() else {
		return None;
	};
	let mut shape = eval_shape(files, doc, &value, None, 1)?;
	for s in &path {
		shape = step(files, shape, s, 0)?;
	}
	let Shape::Object(fields) = shape else {
		return None;
	};
	fields.into_iter().find(|f| f.name == name)
}

fn base_shape(
	files: &dyn Files,
	doc: &Rc<Document>,
	node: &SyntaxNode,
	depth: u8,
) -> Option<Shape> {
	let sub = |e: Option<Expr>| eval_shape(files, doc, e?.syntax(), None, depth + 1);
	let object = |fields| Some(Shape::Object(fields));
	match node.kind() {
		SyntaxKind::EXPR_OBJECT => object(object_fields(doc, &ExprObject::cast(node.clone())?)?),
		SyntaxKind::EXPR_ARRAY => Some(Shape::Array(
			node.children()
				.filter(|c| c.kind() == SyntaxKind::EXPR)
				.map(|expr| Elem {
					doc: doc.clone(),
					expr,
				})
				.collect(),
		)),
		SyntaxKind::EXPR_PARENED => sub(ExprParened::cast(node.clone())?.expr()),
		SyntaxKind::EXPR_LITERAL => {
			let kw = node.first_token()?.kind();
			object(keyword_fields(files, doc, node, kw, depth)?)
		}
		SyntaxKind::EXPR_OBJ_EXTEND => {
			let ext = ExprObjExtend::cast(node.clone())?;
			let (Shape::Object(lhs), Shape::Object(rhs)) = (sub(ext.lhs())?, sub(ext.rhs())?)
			else {
				return None;
			};
			object(merge(lhs, rhs))
		}
		SyntaxKind::EXPR_BINARY => {
			let bin = ExprBinary::cast(node.clone())?;
			// The operator is a bare token child; only `+` combines objects and arrays.
			let is_plus = node
				.children_with_tokens()
				.any(|e| e.kind() == SyntaxKind::PLUS);
			if !is_plus {
				return None;
			}
			match (sub(bin.lhs())?, sub(bin.rhs())?) {
				(Shape::Object(lhs), Shape::Object(rhs)) => object(merge(lhs, rhs)),
				(Shape::Array(mut lhs), Shape::Array(rhs)) => {
					lhs.extend(rhs);
					Some(Shape::Array(lhs))
				}
				_ => None,
			}
		}
		SyntaxKind::EXPR_IMPORT => {
			let import = ExprImport::cast(node.clone())?;
			if import.import_kind()?.kind() != ImportKindKind::ImportKw {
				return None;
			}
			let text = import.text()?;
			let path = imports::string_value(text.syntax().kind(), text.syntax().text())?;
			let target = files.import(doc, &path)?;
			let root = target
				.root
				.children()
				.find(|c| c.kind() == SyntaxKind::EXPR)?;
			eval_shape(files, &target, &root, None, depth + 1)
		}
		SyntaxKind::EXPR_VAR => {
			let var = ExprVar::cast(node.clone())?;
			let Resolution::Local(decl) = scope::resolve(&var)? else {
				return None;
			};
			let (value, path) = scope::decl_source(&doc.root, &decl)?;
			let mut shape = eval_shape(files, doc, &value, None, depth + 1)?;
			for s in &path {
				shape = step(files, shape, s, depth)?;
			}
			Some(shape)
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
	let mut exprs = container
		.children()
		.filter(|c| c.kind() == SyntaxKind::EXPR);
	let (lhs, rhs) = (exprs.next()?, exprs.next()?);
	(combines && rhs == wrapper).then_some(lhs)
}

/// Fields of the object literal `obj` as seen by `self`: its own plus those it extends.
fn self_fields(
	files: &dyn Files,
	doc: &Rc<Document>,
	obj: &SyntaxNode,
	depth: u8,
) -> Option<Fields> {
	let own = object_fields(doc, &ExprObject::cast(obj.clone())?)?;
	let inherited = super_expr(obj)
		.and_then(|lhs| eval(files, doc, &lhs, None, depth + 1))
		.unwrap_or_default();
	Some(merge(inherited, own))
}

/// `self`, `super` and `$` in the object literals that enclose `node`.
fn keyword_fields(
	files: &dyn Files,
	doc: &Rc<Document>,
	node: &SyntaxNode,
	kw: SyntaxKind,
	depth: u8,
) -> Option<Fields> {
	let mut objects = node
		.ancestors()
		.filter(|n| n.kind() == SyntaxKind::EXPR_OBJECT);
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
			matches!(
				n.kind(),
				SyntaxKind::MEMBER_FIELD_NORMAL | SyntaxKind::MEMBER_FIELD_METHOD
			)
		})
	}
}
