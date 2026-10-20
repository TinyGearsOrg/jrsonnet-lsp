mod line_index;
mod server;

fn main() -> anyhow::Result<()> {
	server::run()
}
