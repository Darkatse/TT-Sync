use crate::config::UiLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpTab {
    About,
    Keys,
    Tips,
}

impl HelpTab {
    pub const ALL: [HelpTab; 3] = [HelpTab::About, HelpTab::Keys, HelpTab::Tips];

    pub fn title(self, lang: UiLanguage) -> &'static str {
        match (lang, self) {
            (UiLanguage::ZhCn, HelpTab::About) => "关于",
            (UiLanguage::En, HelpTab::About) => "About",
            (UiLanguage::ZhCn, HelpTab::Keys) => "快捷键",
            (UiLanguage::En, HelpTab::Keys) => "Keys",
            (UiLanguage::ZhCn, HelpTab::Tips) => "部署提示",
            (UiLanguage::En, HelpTab::Tips) => "Deploy Tips",
        }
    }
}

pub struct State {
    pub tab: HelpTab,
    pub lang: UiLanguage,
}

impl State {
    pub fn new() -> Self {
        Self {
            tab: HelpTab::About,
            lang: UiLanguage::ZhCn,
        }
    }

    pub fn next_tab(&mut self) {
        self.tab = match self.tab {
            HelpTab::About => HelpTab::Keys,
            HelpTab::Keys => HelpTab::Tips,
            HelpTab::Tips => HelpTab::About,
        };
    }

    pub fn prev_tab(&mut self) {
        self.tab = match self.tab {
            HelpTab::About => HelpTab::Tips,
            HelpTab::Keys => HelpTab::About,
            HelpTab::Tips => HelpTab::Keys,
        };
    }
}

pub fn about_text(lang: UiLanguage) -> Vec<&'static str> {
    match lang {
        UiLanguage::ZhCn => vec![
            "TT-Sync — TauriTavern 远程同步服务端",
            "",
            "开源项目，MIT 协议",
            "GitHub: https://github.com/Darkatse/TT-Sync",
            "",
            "TT-Sync 让你在 VPS / NAS / 家庭服务器上运行一个",
            "安全的同步端点，与 TauriTavern 或 SillyTavern 进行",
            "双向同步角色数据、扩展和配置。",
            "",
            "核心特性：",
            "  • TLS 1.3 + SPKI 证书绑定",
            "  • Ed25519 认证 + 短期 Session Token",
            "  • 增量同步 / 完全镜像模式",
            "  • 跨平台支持 (Linux / macOS / Windows)",
        ],
        UiLanguage::En => vec![
            "TT-Sync — Remote sync server for TauriTavern",
            "",
            "Open source, MIT licensed",
            "GitHub: https://github.com/Darkatse/TT-Sync",
            "",
            "TT-Sync lets you host a secure sync endpoint on a",
            "VPS, NAS, or home server, and bidirectionally sync",
            "character data, extensions, and config with TauriTavern",
            "or SillyTavern.",
            "",
            "Key features:",
            "  • TLS 1.3 + SPKI certificate pinning",
            "  • Ed25519 authentication + short-lived sessions",
            "  • Incremental sync / full mirror mode",
            "  • Cross-platform (Linux / macOS / Windows)",
        ],
    }
}

pub fn keys_text(lang: UiLanguage) -> Vec<&'static str> {
    match lang {
        UiLanguage::ZhCn => vec![
            "全局",
            "  q           退出",
            "  Esc         返回上一级",
            "  ↑↓          移动焦点",
            "  Enter       确认",
            "",
            "主菜单",
            "  ↑↓          选择菜单项",
            "  Enter       进入选中项",
            "",
            "Onboard（引导设置）",
            "  ←→          切换选项（语言/是否配对/服务模式）",
            "  Enter       下一步",
            "  Esc         上一步",
            "",
            "配对（Pairing）",
            "  r           刷新二维码 / 重新生成 Token",
            "  ↑↓          权限 / 继续操作选择",
            "",
            "已配对设备（Peers）",
            "  ↑↓          选择设备",
            "  Enter       打开操作菜单",
            "  p           直接调整权限",
            "  d           直接撤销设备",
            "  r           刷新列表",
            "",
            "服务管理（Serve）",
            "  ↑↓          选择操作",
            "  Enter       执行",
            "",
            "帮助 / 关于",
            "  Tab / ←→    切换标签页",
        ],
        UiLanguage::En => vec![
            "Global",
            "  q           Quit",
            "  Esc         Go back",
            "  ↑↓          Move focus",
            "  Enter       Confirm",
            "",
            "Main Menu",
            "  ↑↓          Select item",
            "  Enter       Enter selected",
            "",
            "Onboard",
            "  ←→          Toggle option (language/pair/service)",
            "  Enter       Next step",
            "  Esc         Previous step",
            "",
            "Pairing",
            "  r           Refresh QR / regenerate token",
            "  ↑↓          Permission / continue selection",
            "",
            "Peers",
            "  ↑↓          Select peer",
            "  Enter       Open actions",
            "  p           Edit permissions",
            "  d           Revoke peer",
            "  r           Refresh list",
            "",
            "Serve",
            "  ↑↓          Select action",
            "  Enter       Execute",
            "",
            "Help / About",
            "  Tab / ←→    Switch tabs",
        ],
    }
}

pub fn tips_text(lang: UiLanguage) -> Vec<&'static str> {
    match lang {
        UiLanguage::ZhCn => vec![
            "VPS 部署",
            "  1. 上传 tt-sync 二进制到 VPS",
            "  2. 运行 tt-sync onboard 完成初始化",
            "  3. 确保防火墙已放行选定端口",
            "  4. 建议使用 systemd 管理服务",
            "",
            "NAS 部署",
            "  1. 在 NAS 上部署 tt-sync（或 Docker 容器）",
            "  2. 确保端口映射/端口转发已配置",
            "  3. 使用内网 IP 或 DDNS 域名作为 Public URL",
            "",
            "反向代理（Nginx / Caddy）",
            "  • 将域名反代到 tt-sync 的监听端口",
            "  • Public URL 设置为你的域名 URL",
            "  • TT-Sync 自带 TLS，反代可选择 pass-through",
            "",
            "Docker",
            "  • 使用 sillytavern-docker 布局模式",
            "  • 挂载数据卷到容器内的 workspace 路径",
            "  • 持久化配对与证书数据（Docker 挂载）",
        ],
        UiLanguage::En => vec![
            "VPS Deployment",
            "  1. Upload tt-sync binary to your VPS",
            "  2. Run tt-sync onboard to initialize",
            "  3. Ensure firewall allows the chosen port",
            "  4. Recommended: manage with systemd",
            "",
            "NAS Deployment",
            "  1. Deploy tt-sync on NAS (or Docker container)",
            "  2. Ensure port forwarding is configured",
            "  3. Use LAN IP or DDNS domain as Public URL",
            "",
            "Reverse Proxy (Nginx / Caddy)",
            "  • Proxy your domain to tt-sync's listen port",
            "  • Set Public URL to your domain URL",
            "  • TT-Sync has built-in TLS; proxy can pass-through",
            "",
            "Docker",
            "  • Use sillytavern-docker layout mode",
            "  • Mount data volumes to the workspace path",
            "  • Persist pairing & cert data (docker volume)",
        ],
    }
}
