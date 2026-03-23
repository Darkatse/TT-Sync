use crate::Context;
use crate::config::{self, UiLanguage};
use crate::tui::i18n::tr;

use ttsync_fs::layout::WorkspaceMounts;
use ttsync_http::tls::{SelfManagedTls, TlsProvider};

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub label: &'static str,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

pub struct State {
    pub checks: Vec<CheckResult>,
    pub lang: UiLanguage,
}

impl State {
    pub fn new() -> Self {
        Self {
            checks: Vec::new(),
            lang: UiLanguage::ZhCn,
        }
    }

    pub fn run(&mut self, ctx: &Context, lang: UiLanguage) {
        self.lang = lang;
        self.checks.clear();

        // 1. State dir
        self.checks.push(check_state_dir(ctx, lang));

        // 2. Config
        self.checks.push(check_config(ctx, lang));

        // 3. Identity
        self.checks.push(check_identity(ctx, lang));

        // 4. TLS
        self.checks.push(check_tls(ctx, lang));

        // 5. Peers
        self.checks.push(check_peers(ctx, lang));

        // 6. Workspace mounts
        self.checks.push(check_workspace(ctx, lang));
    }
}

fn check_state_dir(ctx: &Context, lang: UiLanguage) -> CheckResult {
    let label = tr(lang, "状态目录", "State dir");
    if ctx.state_dir.is_dir() {
        CheckResult {
            label,
            status: CheckStatus::Ok,
            detail: ctx.state_dir.display().to_string(),
        }
    } else {
        CheckResult {
            label,
            status: CheckStatus::Fail,
            detail: tr(
                lang,
                "状态目录不存在，请先运行 Onboard",
                "State dir does not exist; run Onboard first",
            )
            .to_owned(),
        }
    }
}

fn check_config(ctx: &Context, lang: UiLanguage) -> CheckResult {
    let label = tr(lang, "配置文件", "Config");
    if !ctx.config_path.exists() {
        return CheckResult {
            label,
            status: CheckStatus::Fail,
            detail: tr(lang, "config.toml 不存在", "config.toml not found").to_owned(),
        };
    }
    match config::load_config(&ctx.config_path) {
        Ok(_) => CheckResult {
            label,
            status: CheckStatus::Ok,
            detail: ctx.config_path.display().to_string(),
        },
        Err(e) => CheckResult {
            label,
            status: CheckStatus::Fail,
            detail: e.to_string(),
        },
    }
}

fn check_identity(ctx: &Context, lang: UiLanguage) -> CheckResult {
    let label = tr(lang, "身份密钥", "Identity");
    let path = config::identity_path(&ctx.state_dir);
    if !path.exists() {
        return CheckResult {
            label,
            status: CheckStatus::Warn,
            detail: tr(
                lang,
                "identity.json 不存在（Onboard 将自动创建）",
                "identity.json missing (Onboard will create it)",
            )
            .to_owned(),
        };
    }
    match config::load_identity(&ctx.state_dir) {
        Ok(_) => CheckResult {
            label,
            status: CheckStatus::Ok,
            detail: path.display().to_string(),
        },
        Err(e) => CheckResult {
            label,
            status: CheckStatus::Fail,
            detail: e.to_string(),
        },
    }
}

fn check_tls(ctx: &Context, lang: UiLanguage) -> CheckResult {
    let label = tr(lang, "TLS 证书", "TLS cert");
    match SelfManagedTls::load_or_create(&ctx.state_dir) {
        Ok(tls) => CheckResult {
            label,
            status: CheckStatus::Ok,
            detail: format!("spki: {}", tls.spki_sha256()),
        },
        Err(e) => CheckResult {
            label,
            status: CheckStatus::Fail,
            detail: e.to_string(),
        },
    }
}

fn check_peers(ctx: &Context, lang: UiLanguage) -> CheckResult {
    let label = tr(lang, "已配对设备", "Peers");
    let path = ctx.state_dir.join("peers.json");
    if !path.exists() {
        return CheckResult {
            label,
            status: CheckStatus::Ok,
            detail: tr(lang, "暂无设备（peers.json 尚未创建）", "No peers yet (peers.json not created)").to_owned(),
        };
    }
    match std::fs::read_to_string(&path).and_then(|s| {
        serde_json::from_str::<Vec<serde_json::Value>>(&s)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }) {
        Ok(peers) => CheckResult {
            label,
            status: CheckStatus::Ok,
            detail: format!(
                "{} {}",
                peers.len(),
                tr(lang, "台设备", "peer(s)")
            ),
        },
        Err(e) => CheckResult {
            label,
            status: CheckStatus::Fail,
            detail: e.to_string(),
        },
    }
}

fn check_workspace(ctx: &Context, lang: UiLanguage) -> CheckResult {
    let label = tr(lang, "同步文件夹", "Workspace");
    if !ctx.config_path.exists() {
        return CheckResult {
            label,
            status: CheckStatus::Warn,
            detail: tr(lang, "配置不存在，跳过检测", "Config missing, skipped").to_owned(),
        };
    }
    let cfg = match config::load_config(&ctx.config_path) {
        Ok(c) => c,
        Err(e) => {
            return CheckResult {
                label,
                status: CheckStatus::Fail,
                detail: e.to_string(),
            }
        }
    };
    if !cfg.workspace_path.exists() {
        return CheckResult {
            label,
            status: CheckStatus::Fail,
            detail: format!(
                "{}: {}",
                tr(lang, "路径不存在", "Path does not exist"),
                cfg.workspace_path.display()
            ),
        };
    }
    match WorkspaceMounts::derive(cfg.layout, &cfg.workspace_path) {
        Ok(m) => CheckResult {
            label,
            status: CheckStatus::Ok,
            detail: format!("data: {}", m.data_root.display()),
        },
        Err(e) => CheckResult {
            label,
            status: CheckStatus::Fail,
            detail: e.to_string(),
        },
    }
}
