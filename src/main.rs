mod line_index;
mod imports;
mod scope;
mod uri;
mod server;

fn main() -> anyhow::Result<()> {
	server::run()
}
