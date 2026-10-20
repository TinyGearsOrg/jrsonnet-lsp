mod document;
mod fields;
mod imports;
mod line_index;
mod scope;
mod server;
mod signature;
mod stdlib;
mod symbols;
mod uri;

fn main() -> anyhow::Result<()> {
	server::run()
}
