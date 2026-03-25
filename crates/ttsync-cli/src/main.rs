mod commands;
mod config;
mod launch_agent;
mod output;
mod server_runtime;
mod systemd;
mod tui;
mod user_service;
mod windows_background;
mod windows_task_scheduler;

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

    /// Override the config file path for CLI subcommands. Ignored by TUI entrypoints.
    #[arg(long, global = true, value_name = "PATH")]
    config_file: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<commands::Command>,
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
    pub config_path: PathBuf,
    pub json: bool,
}

impl Context {
    fn new(use_color: bool, quiet: bool, state_dir: PathBuf, config_path: PathBuf) -> Self {
        Self {
            style: Style::new(use_color),
            quiet,
            state_dir,
            config_path,
            json: false,
        }
    }
}

#[tokio::main]
async fn main() {
    let Cli {
        no_color,
        quiet,
        state_dir: state_dir_override,
        config_file,
        command,
    } = Cli::parse();

    let use_color = !no_color && supports_color();

    // Only enable tracing for `serve` (long-running) or when RUST_LOG is set.
    if matches!(command.as_ref(), Some(commands::Command::Serve))
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

    let default_config_path = match config::default_config_path() {
        Ok(path) => path,
        Err(e) => {
            let s = Style::new(use_color);
            eprintln!("{} {}", s.bold_red("error:"), e);
            std::process::exit(1);
        }
    };
    let state_dir = config::state_dir(state_dir_override.as_deref());

    let result = match command {
        Some(command) => {
            let ctx = Context::new(
                use_color,
                quiet,
                state_dir,
                config::resolve_config_path(
                    default_config_path,
                    config_file.as_deref(),
                    command.config_path_mode(),
                ),
            );
            commands::execute(&ctx, command).await
        }
        None => {
            use clap::CommandFactory;
            use std::io::IsTerminal;

            if !std::io::stdout().is_terminal() {
                Cli::command()
                    .print_help()
                    .expect("print help must succeed");
                println!();
                return;
            }

            let ctx = Context::new(use_color, quiet, state_dir, default_config_path);
            tui::run(&ctx)
        }
    };

    if let Err(error) = result {
        let s = Style::new(use_color);
        eprintln!("{} {}", s.bold_red("error:"), error);
        std::process::exit(1);
    }
}

fn supports_color() -> bool {
    std::env::var("NO_COLOR").is_err() && std::env::var("TERM").as_deref() != Ok("dumb")
}
