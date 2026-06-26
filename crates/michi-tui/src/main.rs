use clap::Parser;
use michi_tui::app::App;

#[derive(Parser)]
#[command(name = "michi", about = "Terminal client for Michi Micro Server")]
struct Args {
    #[arg(long, default_value = "localhost")]
    host: String,
    #[arg(long, default_value_t = 8096)]
    port: u16,
    #[arg(long)]
    token: Option<String>,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let base_url = format!("http://{}:{}", args.host, args.port);

    let mut terminal = ratatui::init();
    let mut app = App::new(base_url, args.token);
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
}
