mod commands;
mod config;
mod output;

use std::path::PathBuf;

use clap::Parser;
use output::Style;

/// TT-Sync — Remote synchronization server for TauriTavern
#[derive(Parser)]
#[command(name = "tt-sync", version, about, long_about = None)]
#[command(styles = clap_styles())]
struct Cli {
    /// Disable colored output.
    #[arg(long, global = true)]
    no_color: bool,

    /// Suppress non-essential output.
    #[arg(long, global = true)]
    quiet: bool,

    /// Override the state directory path.
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: commands::Command,
}

fn clap_styles() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .header(clap::builder::styling::AnsiColor::Cyan.on_default().bold())
        .usage(clap::builder::styling::AnsiColor::Cyan.on_default().bold())
        .literal(clap::builder::styling::AnsiColor::Green.on_default())
        .placeholder(clap::builder::styling::AnsiColor::Yellow.on_default())
}

pub struct Context {
    pub style: Style,
    pub quiet: bool,
    pub state_dir: PathBuf,
    pub json: bool,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let use_color = !cli.no_color && supports_color();

    // Only enable tracing for `serve` (long-running) or when RUST_LOG is set.
    if matches!(cli.command, commands::Command::Serve { .. })
        || std::env::var("RUST_LOG").is_ok()
    {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_ansi(use_color)
            .init();
    }

    let ctx = Context {
        style: Style::new(use_color),
        quiet: cli.quiet,
        state_dir: config::state_dir(cli.state_dir.as_deref()),
        json: false,
    };

    if let Err(error) = commands::execute(ctx, cli.command).await {
        let s = Style::new(use_color);
        eprintln!("{} {}", s.bold_red("error:"), error);
        std::process::exit(1);
    }
}

fn supports_color() -> bool {
    std::env::var("NO_COLOR").is_err() && std::env::var("TERM").as_deref() != Ok("dumb")
}
