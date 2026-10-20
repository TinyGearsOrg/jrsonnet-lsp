mod line_index;
mod document;
mod fields;
mod imports;
mod scope;
mod stdlib;
mod symbols;
mod uri;
mod server;

fn main() -> anyhow::Result<()> {
	server::run()
}
