use ratatui::widgets::ListState;

use crate::config::UiLanguage;
use crate::server_runtime::RunningServer;
use crate::tui::doctor;
use crate::tui::help;
use crate::tui::onboard;
use crate::tui::pairing;
use crate::tui::peers;
use crate::tui::serve;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuItem {
    Onboard,
    Pair,
    Peers,
    Serve,
    Doctor,
    Help,
    Exit,
}

impl MainMenuItem {
    pub const ALL: [MainMenuItem; 7] = [
        MainMenuItem::Onboard,
        MainMenuItem::Pair,
        MainMenuItem::Peers,
        MainMenuItem::Serve,
        MainMenuItem::Doctor,
        MainMenuItem::Help,
        MainMenuItem::Exit,
    ];

    pub fn title(self, lang: UiLanguage) -> &'static str {
        match (lang, self) {
            (UiLanguage::ZhCn, MainMenuItem::Onboard) => "Onboard（引导设置）",
            (UiLanguage::ZhCn, MainMenuItem::Pair) => "开始配对（生成二维码/链接）",
            (UiLanguage::ZhCn, MainMenuItem::Peers) => "已配对设备（列表/撤销）",
            (UiLanguage::ZhCn, MainMenuItem::Serve) => "启动/停止服务",
            (UiLanguage::ZhCn, MainMenuItem::Doctor) => "运行诊断",
            (UiLanguage::ZhCn, MainMenuItem::Help) => "帮助 / 关于",
            (UiLanguage::ZhCn, MainMenuItem::Exit) => "退出",
            (UiLanguage::En, MainMenuItem::Onboard) => "Onboard (Guided setup)",
            (UiLanguage::En, MainMenuItem::Pair) => "Pair (QR / link)",
            (UiLanguage::En, MainMenuItem::Peers) => "Peers (list / revoke)",
            (UiLanguage::En, MainMenuItem::Serve) => "Start/Stop service",
            (UiLanguage::En, MainMenuItem::Doctor) => "Doctor",
            (UiLanguage::En, MainMenuItem::Help) => "Help / About",
            (UiLanguage::En, MainMenuItem::Exit) => "Exit",
        }
    }
}

pub struct MainMenuState {
    pub list: ListState,
}

impl MainMenuState {
    pub fn new() -> Self {
        let mut list = ListState::default();
        list.select(Some(0));
        Self { list }
    }

    pub fn selected(&self) -> usize {
        self.list
            .selected()
            .expect("main menu selection must be set")
    }

    pub fn selected_item(&self) -> MainMenuItem {
        MainMenuItem::ALL[self.selected()]
    }

    pub fn next(&mut self) {
        let i = self.selected();
        let next = (i + 1) % MainMenuItem::ALL.len();
        self.list.select(Some(next));
    }

    pub fn prev(&mut self) {
        let i = self.selected();
        let prev = (i + MainMenuItem::ALL.len() - 1) % MainMenuItem::ALL.len();
        self.list.select(Some(prev));
    }
}

pub struct PlaceholderState {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingFlow {
    MainMenu,
    Onboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    MainMenu,
    Onboard,
    Pairing,
    Peers,
    Serve,
    Doctor,
    Help,
    #[allow(dead_code)]
    Placeholder,
}

pub struct App {
    pub screen: Screen,
    pub language: UiLanguage,
    pub main_menu: MainMenuState,
    pub onboard: onboard::State,
    pub pairing: pairing::State,
    pub pairing_flow: PairingFlow,
    pub peers: peers::State,
    pub serve: serve::State,
    pub doctor: doctor::State,
    pub help: help::State,
    pub server: Option<RunningServer>,
    pub placeholder: Option<PlaceholderState>,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::MainMenu,
            language: UiLanguage::ZhCn,
            main_menu: MainMenuState::new(),
            onboard: onboard::State::new(),
            pairing: pairing::State::new(),
            pairing_flow: PairingFlow::MainMenu,
            peers: peers::State::new(),
            serve: serve::State::new(),
            doctor: doctor::State::new(),
            help: help::State::new(),
            server: None,
            placeholder: None,
            should_quit: false,
        }
    }

    #[allow(dead_code)]
    pub fn open_placeholder(&mut self, title: impl Into<String>, body: impl Into<String>) {
        self.placeholder = Some(PlaceholderState {
            title: title.into(),
            body: body.into(),
        });
        self.screen = Screen::Placeholder;
    }

    pub fn go_home(&mut self) {
        self.screen = Screen::MainMenu;
        self.placeholder = None;
    }

    pub fn start_onboard(&mut self) {
        self.onboard = onboard::State::new();
        self.onboard.language = self.language;
        self.screen = Screen::Onboard;
    }

    pub fn start_pairing(&mut self, flow: PairingFlow) {
        self.pairing = pairing::State::new();
        self.pairing_flow = flow;
        self.screen = Screen::Pairing;
    }

    pub fn start_peers(&mut self) {
        self.peers = peers::State::new();
        self.screen = Screen::Peers;
    }

    pub fn start_serve(&mut self) {
        self.serve = serve::State::new();
        self.screen = Screen::Serve;
    }

    pub fn start_doctor(&mut self) {
        self.doctor = doctor::State::new();
        self.screen = Screen::Doctor;
    }

    pub fn start_help(&mut self) {
        self.help = help::State::new();
        self.help.lang = self.language;
        self.screen = Screen::Help;
    }
}
