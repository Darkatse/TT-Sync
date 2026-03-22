use std::net::SocketAddr;
use std::sync::Arc;

use chrono::Local;
use clap::Subcommand;
use comfy_table::{presets, Table};

use ttsync_contract::peer::{DeviceId, Permissions};
use ttsync_contract::sync::ScopeProfileId;
use ttsync_core::pairing::{create_pairing_session, PairingConfig};
use ttsync_core::ports::PeerStore;
use ttsync_core::session::{SessionManager, SessionManagerConfig};
use ttsync_fs::manifest_store::FsManifestStore;
use ttsync_fs::peer_store::JsonPeerStore;
use ttsync_http::server::{ServerState, spawn_server};
use ttsync_http::tls::{SelfManagedTls, TlsProvider};

use crate::config::{self, CliError, Config};
use crate::output;
use crate::Context;

// -----------------------------------------------------------------------
// Command definitions
// -----------------------------------------------------------------------

#[derive(Subcommand)]
pub enum Command {
    /// Initialize a new TT-Sync instance.
    Init {
        /// Path to the data root directory to serve.
        #[arg(long)]
        data_root: String,

        /// Public base URL for pair URIs (e.g., https://my-vps:8443).
        #[arg(long)]
        public_url: String,

        /// Root kind: "data-root" (default) or "user-root".
        #[arg(long, default_value = "data-root")]
        root_kind: String,

        /// Listen address (default: 0.0.0.0:8443).
        #[arg(long, default_value = "0.0.0.0:8443")]
        listen: String,
    },

    /// Start the synchronization server.
    Serve,

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

    /// List available sync profiles.
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
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

        /// Scope profile to grant: "default" or "compatible-minimal".
        #[arg(long, default_value = "default")]
        profile: String,

        /// Grant read+write permissions (default: read-only).
        #[arg(long)]
        rw: bool,

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
pub enum ProfileAction {
    /// List available sync profiles with included directories.
    List,
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

pub async fn execute(ctx: Context, command: Command) -> Result<(), CliError> {
    match command {
        Command::Init { data_root, public_url, root_kind, listen } => {
            cmd_init(&ctx, &data_root, &public_url, &root_kind, &listen)
        }
        Command::Serve => cmd_serve(ctx).await,
        Command::Pair { action } => match action {
            PairAction::Open { expires, profile, rw, mirror, json } => {
                cmd_pair_open(&ctx, &expires, &profile, rw, mirror, json)
            }
        },
        Command::Peers { action } => match action {
            PeersAction::List { json } => cmd_peers_list(&ctx, json).await,
            PeersAction::Revoke { peer } => cmd_peers_revoke(&ctx, &peer).await,
        },
        Command::Profile { action } => match action {
            ProfileAction::List => cmd_profile_list(&ctx),
        },
        Command::Doctor => cmd_doctor(&ctx),
        Command::Cert { action } => match action {
            CertAction::Show => cmd_cert_show(&ctx),
            CertAction::RotateLeaf => cmd_cert_rotate_leaf(&ctx),
        },
    }
}

// -----------------------------------------------------------------------
// init
// -----------------------------------------------------------------------

fn cmd_init(ctx: &Context, data_root: &str, public_url: &str, root_kind: &str, listen: &str) -> Result<(), CliError> {
    let s = &ctx.style;

    let root_kind_config = match root_kind {
        "data-root" => config::RootKindConfig::DataRoot,
        "user-root" => config::RootKindConfig::UserRoot,
        other => return Err(CliError::Config(format!("unknown root kind: {other} (expected data-root or user-root)"))),
    };

    let data_root_path = std::path::Path::new(data_root);
    if !data_root_path.exists() {
        return Err(CliError::Config(format!("data root does not exist: {}", data_root_path.display())));
    }

    if config::config_path(&ctx.state_dir).exists() {
        return Err(CliError::Config(format!(
            "already initialized (config exists at {}). Remove the state directory to re-initialize.",
            config::config_path(&ctx.state_dir).display()
        )));
    }

    // 1. Save config.
    let config = Config {
        data_root: data_root_path.canonicalize()?,
        root_kind: root_kind_config,
        public_url: public_url.to_owned(),
        listen: listen.to_owned(),
    };
    config::save_config(&ctx.state_dir, &config)?;

    // 2. Generate identity (Ed25519).
    let identity = config::load_or_create_identity(&ctx.state_dir)?;

    // 3. Generate TLS certificate.
    let tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

    if !ctx.quiet {
        println!();
        println!("  {} TT-Sync initialized", s.bold_green("✓"));
        println!();
        output::print_field(s, "State dir   ", &ctx.state_dir.display().to_string());
        output::print_field(s, "Data root   ", &config.data_root.display().to_string());
        output::print_field(s, "Root kind   ", root_kind);
        output::print_field(s, "Public URL  ", &config.public_url);
        output::print_field(s, "Listen      ", &config.listen);
        output::print_field(s, "Device ID   ", &identity.device_id);
        output::print_field(s, "SPKI SHA-256", tls.spki_sha256());
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

async fn cmd_serve(ctx: Context) -> Result<(), CliError> {
    let config = config::load_config(&ctx.state_dir)?;
    let identity = config::load_or_create_identity(&ctx.state_dir)?;
    let tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

    let device_id = DeviceId::new(identity.device_id.clone())
        .map_err(|e| CliError::Config(e.to_string()))?;

    let manifest_store = Arc::new(FsManifestStore::new(
        config.data_root.clone(),
        config.root_kind.into(),
    ));
    let peer_store = Arc::new(JsonPeerStore::new(ctx.state_dir.clone()));
    let session_manager = Arc::new(SessionManager::new(SessionManagerConfig::default()));

    let state = Arc::new(ServerState::new(
        device_id,
        identity.device_name,
        manifest_store,
        peer_store,
        session_manager,
    ));

    let addr: SocketAddr = config.listen.parse()
        .map_err(|e| CliError::Config(format!("invalid listen address: {e}")))?;

    let tls_arc: Arc<dyn TlsProvider> = Arc::new(tls);
    let handle = spawn_server(addr, tls_arc.clone(), state).await?;

    let s = &ctx.style;
    if !ctx.quiet {
        println!();
        println!(
            "  {} TT-Sync server running",
            s.bold_green("▶"),
        );
        println!();
        output::print_field(s, "Listen      ", &handle.addr.to_string());
        output::print_field(s, "Public URL  ", &config.public_url);
        output::print_field(s, "Data root   ", &config.data_root.display().to_string());
        output::print_field(s, "Root kind   ", &format!("{:?}", config.root_kind));
        output::print_field(s, "TLS         ", "self-managed (SPKI pin)");
        output::print_field(s, "SPKI SHA-256", tls_arc.spki_sha256());
        output::print_field(s, "State dir   ", &ctx.state_dir.display().to_string());
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

fn cmd_pair_open(ctx: &Context, expires: &str, profile: &str, rw: bool, mirror: bool, json: bool) -> Result<(), CliError> {
    let s = &ctx.style;
    let config = config::load_config(&ctx.state_dir)?;
    let tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

    let profile_id = parse_profile(profile)?;
    let expires_secs = parse_duration(expires)?;

    let permissions = Permissions {
        read: true,
        write: rw,
        mirror_delete: mirror,
    };

    let pairing_config = PairingConfig {
        profile: profile_id,
        permissions,
        expires_in_secs: expires_secs,
    };

    let (_session, pair_uri) = create_pairing_session(
        &config.public_url,
        tls.spki_sha256(),
        pairing_config,
    )?;

    if json {
        let out = serde_json::json!({
            "pair_uri": pair_uri.to_uri_string(),
            "expires_at_ms": pair_uri.expires_at_ms,
            "spki_sha256": pair_uri.spki_sha256,
            "profile": format!("{profile_id:?}"),
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
    output::print_field(s, "Profile     ", &format!("{profile_id:?}"));
    output::print_field(s, "Read        ", if permissions.read { "yes" } else { "no" });
    output::print_field(s, "Write       ", if permissions.write { "yes" } else { "no" });
    output::print_field(s, "Mirror del  ", if permissions.mirror_delete { "yes" } else { "no" });
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
    table.set_header(["Device ID", "Name", "Profile", "Permissions", "Paired", "Last Sync"]);

    for peer in &peers {
        let perms = format!(
            "{}{}{}",
            if peer.permissions.read { "R" } else { "-" },
            if peer.permissions.write { "W" } else { "-" },
            if peer.permissions.mirror_delete { "D" } else { "-" },
        );

        table.add_row([
            truncate_id(peer.device_id.as_str()),
            peer.device_name.clone(),
            format!("{:?}", peer.profile),
            perms,
            format_timestamp_ms(peer.paired_at_ms),
            peer.last_sync_ms.map(format_timestamp_ms).unwrap_or_else(|| "never".into()),
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

    let grant = matched.ok_or_else(|| {
        CliError::Config(format!("no peer matches \"{}\"", peer_query))
    })?;

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
// profile list
// -----------------------------------------------------------------------

fn cmd_profile_list(ctx: &Context) -> Result<(), CliError> {
    let s = &ctx.style;
    use ttsync_core::scope;

    println!();
    for (id, label, desc) in [
        (ScopeProfileId::CompatibleMinimal, "compatible-minimal", "Exact equivalent of the v1 LAN Sync whitelist"),
        (ScopeProfileId::Default, "default", "Full TauriTavern user content"),
    ] {
        println!("  {} — {}", s.bold_cyan(label), desc);
        println!();
        println!("    {}:", s.dim("Directories"));
        for dir in scope::included_directories(&id) {
            println!("      {dir}");
        }
        println!("    {}:", s.dim("Files"));
        for file in scope::included_files(&id) {
            println!("      {file}");
        }
        println!();
    }

    println!("  {}: {}", s.dim("Global exclusions"), scope::GLOBAL_EXCLUSIONS.join(", "));
    println!();

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
        output::print_ok(s, &format!("State directory exists: {}", ctx.state_dir.display()));
    } else {
        output::print_err(s, &format!("State directory missing: {}", ctx.state_dir.display()));
        output::print_warn(s, "Run `tt-sync init` to initialize.");
        println!();
        return Ok(());
    }

    // Config
    match config::load_config(&ctx.state_dir) {
        Ok(config) => {
            output::print_ok(s, "config.toml loaded");
            // Data root
            if config.data_root.exists() && config.data_root.is_dir() {
                output::print_ok(s, &format!("Data root accessible: {}", config.data_root.display()));
            } else {
                output::print_err(s, &format!("Data root not accessible: {}", config.data_root.display()));
            }
            // Listen
            match config.listen.parse::<SocketAddr>() {
                Ok(_) => output::print_ok(s, &format!("Listen address valid: {}", config.listen)),
                Err(e) => output::print_err(s, &format!("Listen address invalid: {e}")),
            }
        }
        Err(e) => {
            output::print_err(s, &format!("config.toml: {e}"));
        }
    }

    // Identity
    match config::load_identity(&ctx.state_dir) {
        Ok(identity) => {
            output::print_ok(s, &format!("Identity: {} ({})", identity.device_name, truncate_id(&identity.device_id)));
        }
        Err(e) => output::print_err(s, &format!("identity.json: {e}")),
    }

    // TLS
    match SelfManagedTls::load_or_create(&ctx.state_dir) {
        Ok(tls) => {
            output::print_ok(s, &format!("TLS certificate loaded (SPKI: {})", tls.spki_sha256()));
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
            Ok(text) => match serde_json::from_str::<Vec<ttsync_contract::peer::PeerGrant>>(&text) {
                Ok(peers) => output::print_ok(s, &format!("{} paired peer(s)", peers.len())),
                Err(e) => output::print_err(s, &format!("peers.json parse error: {e}")),
            },
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
    output::print_field(s, "Key file    ", &tls_dir.join("key.pem").display().to_string());
    output::print_field(s, "Cert file   ", &tls_dir.join("cert.pem").display().to_string());
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
        return Err(CliError::Config("TLS key not found. Run `tt-sync init` first.".into()));
    }

    // Read existing key.
    let key_pem = std::fs::read_to_string(&key_path)?;
    let key_pair = rcgen::KeyPair::from_pem(&key_pem)
        .map_err(|e| CliError::Config(format!("parse TLS key: {e}")))?;

    // Generate new cert with same key.
    let params = rcgen::CertificateParams::new(["tt-sync".to_owned(), "localhost".to_owned()])
        .map_err(|e| CliError::Config(e.to_string()))?;
    let cert = params.self_signed(&key_pair)
        .map_err(|e| CliError::Config(e.to_string()))?;

    std::fs::write(&cert_path, cert.pem())?;

    // Verify SPKI is unchanged.
    let tls = SelfManagedTls::load_or_create(&ctx.state_dir)?;

    if !ctx.quiet {
        println!();
        println!("  {} Leaf certificate rotated", s.bold_green("✓"));
        println!();
        output::print_field(s, "SPKI SHA-256", tls.spki_sha256());
        println!();
        println!("  SPKI pin is {} — existing paired clients remain valid.", s.bold_green("unchanged"));
        println!();
    }

    Ok(())
}

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn parse_profile(s: &str) -> Result<ScopeProfileId, CliError> {
    match s {
        "default" | "tauritavern-user" => Ok(ScopeProfileId::Default),
        "compatible-minimal" | "minimal" => Ok(ScopeProfileId::CompatibleMinimal),
        other => Err(CliError::Config(format!("unknown profile: {other}"))),
    }
}

fn parse_duration(s: &str) -> Result<u64, CliError> {
    let s = s.trim();
    let (num, unit) = s.split_at(
        s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()),
    );
    let num: u64 = num.parse().map_err(|_| CliError::Config(format!("invalid duration: {s}")))?;
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
        Some(dt) => dt.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string(),
        None => format!("{ms}"),
    }
}
