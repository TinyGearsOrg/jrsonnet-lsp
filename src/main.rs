mod document;
mod fields;
mod imports;
mod line_index;
mod scope;
mod server;
mod stdlib;
mod symbols;
mod uri;

fn main() -> anyhow::Result<()> {
	server::run()
}
