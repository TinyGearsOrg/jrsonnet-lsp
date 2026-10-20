fn main() {
	let src = std::env::args().nth(1).unwrap();
	let (root, _) = jrsonnet_rowan_parser::parse(&src);
	println!("{:#?}", jrsonnet_rowan_parser::AstNode::syntax(&root));
}
