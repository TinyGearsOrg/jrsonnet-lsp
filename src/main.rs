mod line_index;
mod scope;
mod server;

fn main() -> anyhow::Result<()> {
	server::run()
}
