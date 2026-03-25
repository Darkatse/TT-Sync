use std::net::SocketAddr;
use std::path::Path;

use chrono::Local;
use clap::Subcommand;
use comfy_table::{Table, presets};

use ttsync_contract::peer::Permissions;
use ttsync_core::pairing::{PairingConfig, create_pairing_session};
use ttsync_core::ports::PeerStore;
use ttsync_fs::layout::{LayoutMode, WorkspaceMounts};
use ttsync_fs::peer_store::JsonPeerStore;
use ttsync_http::pairing_store::PairingTokenStore;
use ttsync_http::tls::{SelfManagedTls, TlsProvider};

use crate::Context;
use crate::config::{self, CliError, Config};
use crate::output;
use crate::server_runtime;

// -----------------------------------------------------------------------
// Command definitions
// -----------------------------------------------------------------------

#[derive(Subcommand)]
pub enum Command {
    /// Guided onboarding flow (TUI).
    Onboard,
    /// Initialize a new TT-Sync instance.
    Init {
        /// Workspace path (layout anchor).
        ///
        /// Examples:
        /// - TauriTavern: the `data/` folder
        /// - SillyTavern: the repo root, `data/`, or `data/default-user/`
        #[arg(long)]
        path: String,

        /// Layout mode: tauritavern | sillytavern | sillytavern-docker.
        #[arg(long, default_value = "tauritavern")]
        layout: String,

        /// Public base URL for pair URIs (e.g., https://my-vps:8443).
        #[arg(long)]
        public_url: String,

        /// Listen address (default: 0.0.0.0:8443).
        #[arg(long, default_value = "0.0.0.0:8443")]
        listen: String,
    },

    /// Start the synchronization server.
    Serve,

    /// Windows-only hidden background server entrypoint.
    #[command(hide = true)]
    BackgroundServe,

    /// Manage device pairing.
    Pair {
        #[command(subcommand)]
        action: PairAction,
    },

    /// Manage paired peers.
    Peers {
        #[command(subcommand)]
        action: PeersAction,
    },

    /// Check system health and configuration.
    Doctor,

    /// Manage TLS certificates.
    Cert {
        #[command(subcommand)]
        action: CertAction,
    },
}

#[derive(Subcommand)]
pub enum PairAction {
    /// Generate a one-time pairing token.
    Open {
        /// Expiry duration (e.g., "5m", "1h", "30s").
        #[arg(long, default_value = "10m")]
        expires: String,

        /// Grant read+write permissions (default).
        #[arg(long)]
        rw: bool,

        /// Grant read-only permissions.
        #[arg(long)]
        ro: bool,

        /// Allow mirror-mode deletions.
        #[arg(long)]
        mirror: bool,

        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum PeersAction {
    /// List all paired peers.
    List {
        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Revoke a paired peer by device ID or name.
    Revoke {
        /// Device ID (UUID) or device name.
        peer: String,
    },
}

#[derive(Subcommand)]
pub enum CertAction {
    /// Show TLS certificate information and SPKI fingerprint.
    Show,
    /// Rotate the leaf certificate (re-signs with same key; preserves SPKI pin).
    RotateLeaf,
}

// -----------------------------------------------------------------------
// Dispatch
// -----------------------------------------------------------------------

pub async fn execute(ctx: &Context, command: Command) -> Result<(), CliError> {
    match command {
        Command::Onboard => crate::tui::run_onboard(ctx),
        Command::Init {
            path,
            layout,
            public_url,
            listen,
        } => cmd_init(ctx, &path, &layout, &public_url, &listen),
        Command::Serve => cmd_serve(ctx).await,
        Command::BackgroundServe => crate::windows_background::run(ctx).await,
        Command::Pair { action } => match action {
            PairAction::Open {
                expires,
                rw,
                ro,
                mirror,
                json,
            } => cmd_pair_open(ctx, &expires, ro, rw, mirror, json),
        },
        Command::Peers { action } => match action {
            PeersAction::List { json } => cmd_peers_list(ctx, json).await,
            PeersAction::Revoke { peer } => cmd_peers_revoke(ctx, &peer).await,
        },
        Command::Doctor => cmd_doctor(ctx),
        Command::Cert { action } => match action {
            CertAction::Show => cmd_cert_show(ctx),
            CertAction::RotateLeaf => cmd_cert_rotate_leaf(ctx),
        },
    }
}

// -----------------------------------------------------------------------
// init
// -----------------------------------------------------------------------

fn cmd_init(
    ctx: &Context,
    path: &str,
    layout: &str,
    public_url: &str,
    listen: &str,
) -> Result<(), CliError> {
    let s = &ctx.style;

    if ctx.config_path.exists() {
        return Err(CliError::Config(format!(
            "already initialized (config exists at {}). Remove config.toml to re-initialize.",
            ctx.config_path.display()
        )));
    }

    let layout_mode = parse_layout(layout)?;

    let workspace_path = Path::new(path);
    if !workspace_path.exists() || !workspace_path.is_dir() {
        return Err(CliError::Config(format!(
            "workspace path is not a directory: {}",
            workspace_path.display()
        )));
    }

    let workspace_path = workspace_path.canonicalize()?;
    let mounts = WorkspaceMounts::derive(layout_mode, &workspace_path)?;

    let config = Config {
        workspace_path,
        layout: layout_mode,
        public_url: public_url.to_owned(),
        listen: listen.to_owned(),
        ui: Default::default(),
    };
    config::save_config(&ctx.config_path, &config)?;

    let identity = config::load_or_create_identity(&ctx.state_dir)?;
    let tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

    if !ctx.quiet {
        println!();
        println!("  {} TT-Sync initialized", s.bold_green("✓"));
        println!();
        output::print_field(s, "State dir      ", &ctx.state_dir.display().to_string());
        output::print_field(
            s,
            "Workspace path ",
            &config.workspace_path.display().to_string(),
        );
        output::print_field(s, "Layout         ", layout);
        output::print_field(
            s,
            "Data root      ",
            &mounts.data_root.display().to_string(),
        );
        output::print_field(
            s,
            "Default user   ",
            &mounts.default_user_root.display().to_string(),
        );
        output::print_field(
            s,
            "Extensions root",
            &mounts.extensions_root.display().to_string(),
        );
        output::print_field(s, "Public URL     ", &config.public_url);
        output::print_field(s, "Listen         ", &config.listen);
        output::print_field(s, "Device ID      ", &identity.device_id);
        output::print_field(s, "SPKI SHA-256   ", tls.spki_sha256());
        println!();
        println!("  Next steps:");
        println!("    {} tt-sync serve", s.dim("$"));
        println!("    {} tt-sync pair open --rw", s.dim("$"));
        println!();
    }

    Ok(())
}

// -----------------------------------------------------------------------
// serve
// -----------------------------------------------------------------------

async fn cmd_serve(ctx: &Context) -> Result<(), CliError> {
    let server_runtime::RunningServer {
        handle,
        config,
        mounts,
        device_id,
        device_name,
        spki_sha256,
    } = server_runtime::start_server(ctx).await?;

    let s = &ctx.style;
    if !ctx.quiet {
        println!();
        println!("  {} TT-Sync server running", s.bold_green("▶"));
        println!();
        output::print_field(s, "Listen         ", &handle.addr.to_string());
        output::print_field(s, "Public URL     ", &config.public_url);
        output::print_field(s, "Device name    ", &device_name);
        output::print_field(s, "Device ID      ", &device_id);
        output::print_field(
            s,
            "Workspace path ",
            &config.workspace_path.display().to_string(),
        );
        output::print_field(s, "Layout         ", &format!("{:?}", config.layout));
        output::print_field(
            s,
            "Data root      ",
            &mounts.data_root.display().to_string(),
        );
        output::print_field(
            s,
            "Default user   ",
            &mounts.default_user_root.display().to_string(),
        );
        output::print_field(
            s,
            "Extensions root",
            &mounts.extensions_root.display().to_string(),
        );
        output::print_field(s, "TLS            ", "self-managed (SPKI pin)");
        output::print_field(s, "SPKI SHA-256   ", &spki_sha256);
        output::print_field(s, "State dir      ", &ctx.state_dir.display().to_string());
        println!();
        println!("  Press {} to stop.", s.dim("Ctrl+C"));
        println!();
    }

    tokio::signal::ctrl_c().await.ok();

    if !ctx.quiet {
        println!();
        println!("  {} Shutting down...", s.dim("•"));
    }
    handle.shutdown();
    Ok(())
}

// -----------------------------------------------------------------------
// pair open
// -----------------------------------------------------------------------

fn cmd_pair_open(
    ctx: &Context,
    expires: &str,
    ro: bool,
    rw: bool,
    mirror: bool,
    json: bool,
) -> Result<(), CliError> {
    let s = &ctx.style;
    let config = config::load_config(&ctx.config_path)?;
    let tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

    let expires_secs = parse_duration(expires)?;

    if ro && rw {
        return Err(CliError::Config(
            "invalid permissions: do not use both --ro and --rw".into(),
        ));
    }
    if ro && mirror {
        return Err(CliError::Config(
            "invalid permissions: --mirror requires write access (do not use --ro)".into(),
        ));
    }

    let permissions = Permissions {
        read: true,
        write: !ro,
        mirror_delete: mirror,
    };

    let pairing_config = PairingConfig {
        permissions,
        expires_in_secs: expires_secs,
    };

    let (session, pair_uri) =
        create_pairing_session(&config.public_url, tls.spki_sha256(), pairing_config)?;
    PairingTokenStore::from_state_dir(ctx.state_dir.clone()).insert(&session)?;

    if json {
        let out = serde_json::json!({
            "pair_uri": pair_uri.to_uri_string(),
            "expires_at_ms": pair_uri.expires_at_ms,
            "spki_sha256": pair_uri.spki_sha256,
            "permissions": {
                "read": permissions.read,
                "write": permissions.write,
                "mirror_delete": permissions.mirror_delete,
            },
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
        return Ok(());
    }

    if ctx.quiet {
        println!("{}", pair_uri.to_uri_string());
        return Ok(());
    }

    let expires_at = format_timestamp_ms(pair_uri.expires_at_ms);

    println!();
    println!("  {} Pairing session created", s.bold_green("✓"));
    println!();
    output::print_field(s, "Pair URI    ", &pair_uri.to_uri_string());
    output::print_field(s, "Expires     ", &expires_at);
    output::print_field(
        s,
        "Read        ",
        if permissions.read { "yes" } else { "no" },
    );
    output::print_field(
        s,
        "Write       ",
        if permissions.write { "yes" } else { "no" },
    );
    output::print_field(
        s,
        "Mirror del  ",
        if permissions.mirror_delete {
            "yes"
        } else {
            "no"
        },
    );
    output::print_field(s, "SPKI SHA-256", &pair_uri.spki_sha256);
    println!();
    println!("  Paste the Pair URI into TauriTavern to complete pairing.");
    println!();

    Ok(())
}

// -----------------------------------------------------------------------
// peers list / revoke
// -----------------------------------------------------------------------

async fn cmd_peers_list(ctx: &Context, json: bool) -> Result<(), CliError> {
    let s = &ctx.style;
    let peer_store = JsonPeerStore::new(ctx.state_dir.clone());
    let peers = peer_store.list_peers().await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&peers).unwrap());
        return Ok(());
    }

    if peers.is_empty() {
        if !ctx.quiet {
            println!();
            println!("  {} No paired peers.", s.dim("•"));
            println!();
        }
        return Ok(());
    }

    let mut table = Table::new();
    table.load_preset(presets::UTF8_FULL_CONDENSED);
    table.set_header(["Device ID", "Name", "Permissions", "Paired", "Last Sync"]);

    for peer in &peers {
        let perms = format!(
            "{}{}{}",
            if peer.permissions.read { "R" } else { "-" },
            if peer.permissions.write { "W" } else { "-" },
            if peer.permissions.mirror_delete {
                "D"
            } else {
                "-"
            },
        );

        table.add_row([
            truncate_id(peer.device_id.as_str()),
            peer.device_name.clone(),
            perms,
            format_timestamp_ms(peer.paired_at_ms),
            peer.last_sync_ms
                .map(format_timestamp_ms)
                .unwrap_or_else(|| "never".into()),
        ]);
    }

    println!();
    println!("{table}");
    println!();

    Ok(())
}

async fn cmd_peers_revoke(ctx: &Context, peer_query: &str) -> Result<(), CliError> {
    let s = &ctx.style;
    let peer_store = JsonPeerStore::new(ctx.state_dir.clone());
    let peers = peer_store.list_peers().await?;

    let matched = peers.iter().find(|p| {
        p.device_id.as_str() == peer_query
            || p.device_id.as_str().starts_with(peer_query)
            || p.device_name.eq_ignore_ascii_case(peer_query)
    });

    let grant =
        matched.ok_or_else(|| CliError::Config(format!("no peer matches \"{}\"", peer_query)))?;

    let device_id = grant.device_id.clone();
    let device_name = grant.device_name.clone();
    peer_store.remove_peer(&device_id).await?;

    if !ctx.quiet {
        println!();
        println!(
            "  {} Revoked peer {} ({})",
            s.bold_green("✓"),
            s.bold(&device_name),
            truncate_id(device_id.as_str()),
        );
        println!();
    }

    Ok(())
}

// -----------------------------------------------------------------------
// doctor
// -----------------------------------------------------------------------

fn cmd_doctor(ctx: &Context) -> Result<(), CliError> {
    let s = &ctx.style;

    println!();
    println!("  {}", s.bold("TT-Sync Doctor"));
    println!();

    // State dir
    if ctx.state_dir.exists() {
        output::print_ok(
            s,
            &format!("State directory exists: {}", ctx.state_dir.display()),
        );
    } else {
        output::print_err(
            s,
            &format!("State directory missing: {}", ctx.state_dir.display()),
        );
        output::print_warn(s, "Run `tt-sync init` to initialize.");
        println!();
        return Ok(());
    }

    // Config
    match config::load_config(&ctx.config_path) {
        Ok(config) => {
            output::print_ok(s, "config.toml loaded");
            output::print_ok(s, &format!("Layout: {:?}", config.layout));
            output::print_ok(
                s,
                &format!("Workspace path: {}", config.workspace_path.display()),
            );

            match WorkspaceMounts::derive(config.layout, &config.workspace_path) {
                Ok(mounts) => {
                    output::print_ok(s, &format!("Data root: {}", mounts.data_root.display()));
                    output::print_ok(
                        s,
                        &format!("Default user: {}", mounts.default_user_root.display()),
                    );
                    output::print_ok(
                        s,
                        &format!("Extensions root: {}", mounts.extensions_root.display()),
                    );
                }
                Err(e) => output::print_err(s, &format!("Mount derivation failed: {e}")),
            }

            match config.listen.parse::<SocketAddr>() {
                Ok(_) => output::print_ok(s, &format!("Listen address valid: {}", config.listen)),
                Err(e) => output::print_err(s, &format!("Listen address invalid: {e}")),
            }
        }
        Err(e) => output::print_err(s, &format!("config.toml: {e}")),
    }

    // Identity
    match config::load_identity(&ctx.state_dir) {
        Ok(identity) => {
            output::print_ok(
                s,
                &format!(
                    "Identity: {} ({})",
                    identity.device_name,
                    truncate_id(&identity.device_id)
                ),
            );
        }
        Err(e) => output::print_err(s, &format!("identity.json: {e}")),
    }

    // TLS
    match SelfManagedTls::load_or_create(&ctx.state_dir) {
        Ok(tls) => {
            output::print_ok(
                s,
                &format!("TLS certificate loaded (SPKI: {})", tls.spki_sha256()),
            );
            match tls.server_config() {
                Ok(_) => output::print_ok(s, "TLS server config builds successfully"),
                Err(e) => output::print_err(s, &format!("TLS server config failed: {e}")),
            }
        }
        Err(e) => output::print_err(s, &format!("TLS: {e}")),
    }

    // Peers
    let peer_path = ctx.state_dir.join("peers.json");
    if peer_path.exists() {
        match std::fs::read_to_string(&peer_path) {
            Ok(text) => {
                match serde_json::from_str::<Vec<ttsync_contract::peer::PeerGrant>>(&text) {
                    Ok(peers) => output::print_ok(s, &format!("{} paired peer(s)", peers.len())),
                    Err(e) => output::print_err(s, &format!("peers.json parse error: {e}")),
                }
            }
            Err(e) => output::print_err(s, &format!("peers.json read error: {e}")),
        }
    } else {
        output::print_ok(s, "No peers.json (0 paired peers)");
    }

    println!();
    Ok(())
}

// -----------------------------------------------------------------------
// cert show / rotate-leaf
// -----------------------------------------------------------------------

fn cmd_cert_show(ctx: &Context) -> Result<(), CliError> {
    let s = &ctx.style;
    let tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

    let tls_dir = ctx.state_dir.join("tls");

    println!();
    println!("  {}", s.bold("TLS Certificate"));
    println!();
    output::print_field(s, "SPKI SHA-256", tls.spki_sha256());
    output::print_field(
        s,
        "Key file    ",
        &tls_dir.join("key.pem").display().to_string(),
    );
    output::print_field(
        s,
        "Cert file   ",
        &tls_dir.join("cert.pem").display().to_string(),
    );
    output::print_field(s, "Mode        ", "self-managed");
    println!();

    Ok(())
}

fn cmd_cert_rotate_leaf(ctx: &Context) -> Result<(), CliError> {
    let s = &ctx.style;
    let tls_dir = ctx.state_dir.join("tls");
    let key_path = tls_dir.join("key.pem");
    let cert_path = tls_dir.join("cert.pem");

    if !key_path.exists() {
        return Err(CliError::Config(
            "TLS key not found. Run `tt-sync init` first.".into(),
        ));
    }

    let key_pem = std::fs::read_to_string(&key_path)?;
    let key_pair = rcgen::KeyPair::from_pem(&key_pem)
        .map_err(|e| CliError::Config(format!("parse TLS key: {e}")))?;

    let params = rcgen::CertificateParams::new(["tt-sync".to_owned(), "localhost".to_owned()])
        .map_err(|e| CliError::Config(e.to_string()))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| CliError::Config(e.to_string()))?;

    std::fs::write(&cert_path, cert.pem())?;

    let tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

    if !ctx.quiet {
        println!();
        println!("  {} Leaf certificate rotated", s.bold_green("✓"));
        println!();
        output::print_field(s, "SPKI SHA-256", tls.spki_sha256());
        println!();
        println!(
            "  SPKI pin is {} — existing paired clients remain valid.",
            s.bold_green("unchanged")
        );
        println!();
    }

    Ok(())
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn parse_layout(s: &str) -> Result<LayoutMode, CliError> {
    match s {
        "tauritavern" => Ok(LayoutMode::TauriTavern),
        "sillytavern" => Ok(LayoutMode::SillyTavern),
        "sillytavern-docker" => Ok(LayoutMode::SillyTavernDocker),
        other => Err(CliError::Config(format!("unknown layout: {other}"))),
    }
}

fn parse_duration(s: &str) -> Result<u64, CliError> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let num: u64 = num
        .parse()
        .map_err(|_| CliError::Config(format!("invalid duration: {s}")))?;
    let multiplier = match unit.trim() {
        "s" | "sec" | "" => 1,
        "m" | "min" => 60,
        "h" | "hr" | "hour" => 3600,
        other => return Err(CliError::Config(format!("unknown duration unit: {other}"))),
    };
    Ok(num * multiplier)
}

fn truncate_id(id: &str) -> String {
    if id.len() > 13 {
        format!("{}…", &id[..12])
    } else {
        id.to_owned()
    }
}

fn format_timestamp_ms(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let naive = chrono::DateTime::from_timestamp(secs, 0);
    match naive {
        Some(dt) => dt
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => format!("{ms}"),
    }
}
