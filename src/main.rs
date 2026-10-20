mod line_index;
mod document;
mod fields;
mod imports;
mod scope;
mod uri;
mod server;

fn main() -> anyhow::Result<()> {
	server::run()
}
