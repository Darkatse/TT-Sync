use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ttsync_fs::layout::LayoutMode;

use crate::Context;
use crate::config::UiLanguage;
use crate::tui::i18n::tr;
use crate::tui::layout as lay;
use crate::tui::onboard::{ServiceMode, State, Step, WorkspacePhase};
use crate::tui::theme;

pub fn render(frame: &mut Frame, ctx: &Context, state: &mut State) {
    let [header, body, footer] = lay::page(frame.area());

    render_header(frame, header, state.language, state.step);
    render_body(frame, ctx, body, state);
    render_footer(
        frame,
        footer,
        state.language,
        state.step,
        state.workspace_phase,
    );
}

fn render_header(frame: &mut Frame, area: ratatui::prelude::Rect, lang: UiLanguage, step: Step) {
    let step_num = match step {
        Step::WelcomeLanguage => 1,
        Step::ListenPort => 2,
        Step::PublicUrl => 3,
        Step::LayoutMode => 4,
        Step::WorkspacePath => 5,
        Step::PairNow => 6,
        Step::ServiceMode => 9,
        Step::Done => 10,
    };

    // Progress bar: ━━●━━━━━━━
    let total = 10;
    let bar: String = (1..=total)
        .map(|i| {
            if i == step_num {
                '●'
            } else if i < step_num {
                '━'
            } else {
                '─'
            }
        })
        .collect();

    lay::render_header(
        frame,
        area,
        vec![
            Span::styled(tr(lang, "Onboard（引导设置）", "Onboard"), theme::title()),
            Span::raw("  "),
            Span::styled(bar, theme::selected()),
            Span::raw("  "),
            Span::styled(format!("{}/{}", step_num, total), theme::hint()),
        ],
    );
}

fn render_body(frame: &mut Frame, ctx: &Context, area: ratatui::prelude::Rect, state: &mut State) {
    match state.step {
        Step::WelcomeLanguage => render_step_language(frame, area, state),
        Step::ListenPort => render_step_port(frame, area, state),
        Step::PublicUrl => render_step_public_url(frame, area, state),
        Step::LayoutMode => render_step_layout(frame, area, state),
        Step::WorkspacePath => render_step_workspace(frame, ctx, area, state),
        Step::PairNow => render_step_pair_now(frame, area, state),
        Step::ServiceMode => render_step_service_mode(frame, ctx, area, state),
        Step::Done => render_step_done(frame, ctx, area, state),
    }
}

fn render_step_language(frame: &mut Frame, area: ratatui::prelude::Rect, state: &State) {
    let [top, mid, bottom] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .areas(area);

    let logo_lines: Vec<Line<'_>> = [
        "████████╗████████╗      ███████╗██╗   ██╗███╗   ██╗ ██████╗",
        "╚══██╔══╝╚══██╔══╝      ██╔════╝╚██╗ ██╔╝████╗  ██║██╔════╝",
        "   ██║      ██║   █████╗███████╗ ╚████╔╝ ██╔██╗ ██║██║     ",
        "   ██║      ██║   ╚════╝╚════██║  ╚██╔╝  ██║╚██╗██║██║     ",
        "   ██║      ██║         ███████║   ██║   ██║ ╚████║╚██████╗",
        "   ╚═╝      ╚═╝         ╚══════╝   ╚═╝   ╚═╝  ╚═══╝ ╚═════╝",
    ]
    .into_iter()
    .map(|s| Line::from(Span::styled(s, theme::brand())))
    .collect();

    frame.render_widget(Paragraph::new(logo_lines), top);

    const PHRASES_ZH: [&str; 7] = [
        "我要给我的角色们一个更大的家！",
        "App已在身后，服务器尽在眼前。",
        "TT-Sync正在运行。请坐和放宽。",
        "你在这里很安全。我会照顾好你的数据。——Seraphina",
        "人生苦短，我用TT。",
        "请问您今天要来点TauriTavern吗？",
        "准备好了吗？让我们开始吧。",
    ];
    const PHRASES_EN: [&str; 7] = [
        "I wanna give my characters a bigger home!",
        "App is behind, Server is ahead.",
        "TT-Sync is running. Please sit tight.",
        "You're safe here. I'll look after you. --Seraphina",
        "Life is short. I use TT.",
        "Would you like to try our TauriTavern today?",
        "Ready? Let’s begin.",
    ];

    let phrases = match state.language {
        UiLanguage::ZhCn => &PHRASES_ZH,
        UiLanguage::En => &PHRASES_EN,
    };
    let phrase = phrases[state.welcome_phrase_idx % phrases.len()];
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(phrase, theme::title())),
            Line::from(""),
            Line::from(tr(
                state.language,
                "请选择语言（可随时在配置中修改）：",
                "Choose your language (can be changed later):",
            )),
        ])
        .wrap(Wrap { trim: true }),
        mid,
    );

    let zh = match state.language {
        UiLanguage::ZhCn => "● 中文",
        UiLanguage::En => "○ 中文",
    };
    let en = match state.language {
        UiLanguage::ZhCn => "○ English",
        UiLanguage::En => "● English",
    };
    frame.render_widget(
        Paragraph::new(Line::from(format!("{zh}    {en}"))).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::BORDER)
                .title(tr(state.language, "语言", "Language")),
        ),
        bottom,
    );
}

fn render_step_port(frame: &mut Frame, area: ratatui::prelude::Rect, state: &State) {
    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .areas(area);

    let input = Paragraph::new(state.port.visualize()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(theme::BORDER)
            .title(tr(
                state.language,
                "监听端口（Enter 下一步）",
                "Listen Port (Enter to continue)",
            )),
    );
    frame.render_widget(input, left);

    let mut help = vec![
        Line::from(Span::styled(
            tr(state.language, "说明", "Notes"),
            theme::title(),
        )),
        Line::from(""),
        Line::from(tr(
            state.language,
            "建议默认 8443（无需管理员权限）。",
            "Recommended: 8443 (no admin/root required).",
        )),
        Line::from(tr(
            state.language,
            "低端口（<1024）在 Linux 上可能需要 root。",
            "Ports <1024 may require root on Linux.",
        )),
    ];

    if let Some(err) = &state.error {
        help.push(Line::from(""));
        help.push(Line::from(Span::styled(err, theme::error())));
    }

    frame.render_widget(
        Paragraph::new(help)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(state.language, "提示", "Help")),
            )
            .wrap(Wrap { trim: true }),
        right,
    );
}

fn render_step_public_url(frame: &mut Frame, area: ratatui::prelude::Rect, state: &State) {
    let [top, bottom] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .areas(area);

    frame.render_widget(
        Paragraph::new(tr(
            state.language,
            "确认 Public URL（用于配对二维码/链接，可编辑后 Enter 下一步）",
            "Confirm Public URL (used in pair links/QR; edit and press Enter)",
        ))
        .wrap(Wrap { trim: true }),
        top,
    );

    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .areas(bottom);

    frame.render_widget(
        Paragraph::new(state.public_url.visualize())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title("Public URL"),
            )
            .wrap(Wrap { trim: false }),
        left,
    );

    let mut info = vec![
        Line::from(Span::styled(
            tr(state.language, "说明", "Notes"),
            theme::title(),
        )),
        Line::from(""),
        Line::from(tr(
            state.language,
            "默认根据本机 IP + 端口生成。",
            "Default is derived from a local IP + port.",
        )),
        Line::from(tr(
            state.language,
            "如果你使用域名/反代，请在此改为你的外网地址。",
            "If you use a domain/reverse proxy, replace it with your public endpoint.",
        )),
        Line::from(tr(
            state.language,
            "不会自动联网查询公网 IP。",
            "No online public-IP lookup is performed.",
        )),
    ];

    if let Some(err) = &state.error {
        info.push(Line::from(""));
        info.push(Line::from(Span::styled(err, theme::error())));
    }

    frame.render_widget(
        Paragraph::new(info)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(state.language, "提示", "Help")),
            )
            .wrap(Wrap { trim: true }),
        right,
    );
}

fn render_step_layout(frame: &mut Frame, area: ratatui::prelude::Rect, state: &mut State) {
    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .areas(area);

    let idx = state
        .layout_list
        .selected()
        .expect("layout selection must be set");
    let labels = [
        (
            "tauritavern",
            tr(state.language, "TauriTavern 数据目录", "TauriTavern data/"),
        ),
        (
            "sillytavern",
            tr(
                state.language,
                "SillyTavern 仓库布局",
                "SillyTavern repo layout",
            ),
        ),
        (
            "sillytavern-docker",
            tr(
                state.language,
                "SillyTavern Docker 卷布局",
                "SillyTavern docker volume",
            ),
        ),
    ];

    let items: Vec<ListItem> = labels
        .iter()
        .enumerate()
        .map(|(i, (id, desc))| {
            let dot = if i == idx { "●" } else { "○" };
            ListItem::new(format!("{dot} {id} — {desc}"))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::BORDER)
                .title(tr(
                    state.language,
                    "Layout 模式（Enter 下一步）",
                    "Layout Mode (Enter to continue)",
                )),
        )
        .highlight_style(theme::selected())
        .highlight_symbol(" ");

    frame.render_stateful_widget(list, left, &mut state.layout_list);

    let detail = layout_detail(state.language, state.layout_mode());
    frame.render_widget(
        Paragraph::new(detail)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(state.language, "对比", "Details")),
            )
            .wrap(Wrap { trim: false }),
        right,
    );
}

fn render_step_workspace(
    frame: &mut Frame,
    ctx: &Context,
    area: ratatui::prelude::Rect,
    state: &State,
) {
    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .areas(area);

    let title = match state.workspace_phase {
        WorkspacePhase::Editing => tr(
            state.language,
            "同步文件夹路径（Enter 检测）",
            "Workspace Path (Enter to detect)",
        ),
        WorkspacePhase::Confirm => tr(
            state.language,
            "同步文件夹路径（Enter 写入配置）",
            "Workspace Path (Enter to write config)",
        ),
    };

    frame.render_widget(
        Paragraph::new(state.workspace_path.visualize())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(title),
            )
            .wrap(Wrap { trim: false }),
        left,
    );

    let mut info = Vec::new();
    info.push(Line::from(Span::styled(
        tr(state.language, "检测结果", "Detection"),
        theme::title(),
    )));
    info.push(Line::from(""));
    info.push(Line::from(format!("layout: {:?}", state.layout_mode())));

    match &state.mounts {
        Some(m) => {
            info.push(Line::from(""));
            info.push(Line::from(vec![
                Span::styled("✓ ", theme::success()),
                Span::raw(format!("data root      : {}", m.data_root.display())),
            ]));
            info.push(Line::from(vec![
                Span::styled("✓ ", theme::success()),
                Span::raw(format!(
                    "default user   : {}",
                    m.default_user_root.display()
                )),
            ]));
            info.push(Line::from(vec![
                Span::styled("✓ ", theme::success()),
                Span::raw(format!("extensions root: {}", m.extensions_root.display())),
            ]));
            info.push(Line::from(""));
            info.push(Line::from(Span::styled(
                tr(
                    state.language,
                    "将写入配置到：",
                    "Config will be written to:",
                ),
                theme::title(),
            )));
            info.push(Line::from(format!("{}", ctx.config_path.display())));
        }
        None => {
            info.push(Line::from(""));
            info.push(Line::from(tr(
                state.language,
                "尚未检测。输入路径后按 Enter。",
                "Not detected yet. Enter a path and press Enter.",
            )));
        }
    }

    if let Some(err) = &state.error {
        info.push(Line::from(""));
        info.push(Line::from(Span::styled(err, theme::error())));
    }

    frame.render_widget(
        Paragraph::new(info)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(state.language, "信息", "Info")),
            )
            .wrap(Wrap { trim: true }),
        right,
    );
}

fn render_step_pair_now(frame: &mut Frame, area: ratatui::prelude::Rect, state: &State) {
    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .areas(area);

    let yes = if state.pair_now {
        tr(
            state.language,
            "● 现在配对（推荐）",
            "● Pair now (recommended)",
        )
    } else {
        tr(
            state.language,
            "○ 现在配对（推荐）",
            "○ Pair now (recommended)",
        )
    };
    let no = if state.pair_now {
        tr(state.language, "○ 稍后再说", "○ Not now")
    } else {
        tr(state.language, "● 稍后再说", "● Not now")
    };

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                tr(state.language, "现在进行配对？", "Pair now?"),
                theme::title(),
            )),
            Line::from(""),
            Line::from(tr(
                state.language,
                "你将获得一个二维码/链接，用于在 TauriTavern 客户端完成配对。",
                "You'll get a QR/link to complete pairing in the TauriTavern client.",
            )),
            Line::from(""),
            Line::from(format!("{yes}    {no}")),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::BORDER)
                .title(tr(state.language, "配对", "Pairing")),
        )
        .wrap(Wrap { trim: true }),
        left,
    );

    let notes = vec![
        Line::from(Span::styled(
            tr(state.language, "说明", "Notes"),
            theme::title(),
        )),
        Line::from(""),
        Line::from(tr(
            state.language,
            "配对需要服务端可被客户端访问（端口/反代/域名）。",
            "Pairing requires the server to be reachable by the client (port/proxy/domain).",
        )),
        Line::from(tr(
            state.language,
            "配对成功后会弹出权限确认（默认：读写，不允许 mirror delete）。",
            "After pairing, you'll confirm permissions (default: read+write, no mirror delete).",
        )),
        Line::from(tr(
            state.language,
            "你也可以稍后在主菜单选择「开始配对」。",
            "You can also do this later from the main menu.",
        )),
    ];

    frame.render_widget(
        Paragraph::new(notes)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(state.language, "提示", "Help")),
            )
            .wrap(Wrap { trim: true }),
        right,
    );
}

fn render_step_service_mode(
    frame: &mut Frame,
    ctx: &Context,
    area: ratatui::prelude::Rect,
    state: &State,
) {
    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .areas(area);

    let is_linux = cfg!(target_os = "linux");

    let (a, b) = if is_linux {
        let systemd_dot = if state.service_mode == ServiceMode::SystemdUser {
            "●"
        } else {
            "○"
        };
        let fg_dot = if state.service_mode == ServiceMode::Foreground {
            "●"
        } else {
            "○"
        };
        (
            format!(
                "{systemd_dot} {}",
                tr(
                    state.language,
                    "安装为 systemd user service（推荐）",
                    "Install as systemd user service (recommended)",
                )
            ),
            format!(
                "{fg_dot} {}",
                tr(
                    state.language,
                    "不安装，直接在本终端运行",
                    "Run in this terminal (no service)",
                )
            ),
        )
    } else {
        (
            format!(
                "● {}",
                tr(
                    state.language,
                    "在本终端运行（当前系统不支持 systemd）",
                    "Run in this terminal (systemd not supported)",
                )
            ),
            String::new(),
        )
    };

    let mut content = vec![
        Line::from(Span::styled(
            tr(state.language, "服务管理", "Service management"),
            theme::title(),
        )),
        Line::from(""),
        Line::from(tr(
            state.language,
            "最后一步：选择如何让 TT-Sync 长期运行。",
            "Final step: choose how TT-Sync keeps running.",
        )),
        Line::from(""),
        Line::from(a),
    ];
    if is_linux {
        content.push(Line::from(""));
        content.push(Line::from(b));
    }

    frame.render_widget(
        Paragraph::new(content)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(state.language, "选择", "Choose")),
            )
            .wrap(Wrap { trim: true }),
        left,
    );

    let mut notes = vec![
        Line::from(Span::styled(
            tr(state.language, "说明", "Notes"),
            theme::title(),
        )),
        Line::from(""),
        Line::from(tr(
            state.language,
            "Onboard 完成后将自动启动服务。",
            "Onboard will auto-start the server.",
        )),
        Line::from(tr(
            state.language,
            "Windows 默认采用「本终端运行」。",
            "Windows defaults to in-terminal run.",
        )),
        Line::from(""),
        Line::from(Span::styled(
            tr(state.language, "路径", "Paths"),
            theme::title(),
        )),
        Line::from(format!(
            "{}: {}",
            tr(state.language, "同步文件夹", "Sync folder"),
            super::sync_folder_display(ctx, state.language)
        )),
        Line::from(format!(
            "{}: {}",
            tr(state.language, "配置文件", "Config"),
            ctx.config_path.display()
        )),
    ];

    if let Some(err) = &state.error {
        notes.push(Line::from(""));
        notes.push(Line::from(Span::styled(err, theme::error())));
    }

    frame.render_widget(
        Paragraph::new(notes)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(state.language, "提示", "Help")),
            )
            .wrap(Wrap { trim: true }),
        right,
    );
}

fn render_step_done(frame: &mut Frame, ctx: &Context, area: ratatui::prelude::Rect, state: &State) {
    let phrase = tr(
        state.language,
        "云端就绪。去把你的角色们接回来吧！",
        "Cloud-ready. Go bring your characters back!",
    );

    let mode = match state.service_mode {
        ServiceMode::Foreground => tr(state.language, "本进程运行", "In-process"),
        ServiceMode::SystemdUser => tr(
            state.language,
            "systemd user service",
            "systemd user service",
        ),
    };

    let body = vec![
        Line::from(Span::styled(
            tr(state.language, "完成！", "Done!"),
            theme::success(),
        )),
        Line::from(""),
        Line::from(phrase),
        Line::from(""),
        Line::from(Span::styled(
            tr(state.language, "摘要", "Summary"),
            theme::title(),
        )),
        Line::from(format!("mode  : {mode}")),
        Line::from(format!(
            "{}: {}",
            tr(state.language, "同步文件夹", "Sync folder"),
            super::sync_folder_display(ctx, state.language)
        )),
        Line::from(format!(
            "{}: {}",
            tr(state.language, "配置文件", "Config"),
            ctx.config_path.display()
        )),
        Line::from(""),
        Line::from(tr(
            state.language,
            "提示：你可以返回主菜单进行配对/管理设备/启停服务。",
            "Tip: Return to the main menu to pair/manage peers/start-stop the server.",
        )),
    ];

    frame.render_widget(
        Paragraph::new(body)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER)
                    .title(tr(state.language, "Onboard 完成", "Onboard completed")),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_footer(
    frame: &mut Frame,
    area: ratatui::prelude::Rect,
    lang: UiLanguage,
    step: Step,
    workspace_phase: WorkspacePhase,
) {
    let hint = match step {
        Step::WelcomeLanguage => tr(
            lang,
            "←→ 选择  Enter 下一步  q 退出",
            "←→ select  Enter next  q quit",
        ),
        Step::ListenPort => tr(
            lang,
            "输入端口  Enter 下一步  Esc 返回  q 退出",
            "type port  Enter next  Esc back  q quit",
        ),
        Step::PublicUrl => tr(
            lang,
            "编辑 URL  Enter 下一步  Esc 返回  q 退出",
            "edit URL  Enter next  Esc back  q quit",
        ),
        Step::LayoutMode => tr(
            lang,
            "↑↓ 选择  Enter 下一步  Esc 返回  q 退出",
            "↑↓ select  Enter next  Esc back  q quit",
        ),
        Step::WorkspacePath => match workspace_phase {
            WorkspacePhase::Editing => tr(
                lang,
                "输入路径  Enter 检测  Esc 返回  q 退出",
                "type path  Enter detect  Esc back  q quit",
            ),
            WorkspacePhase::Confirm => tr(
                lang,
                "Enter 写入配置  Esc 修改路径  q 退出",
                "Enter write  Esc edit  q quit",
            ),
        },
        Step::PairNow => tr(
            lang,
            "←→ 选择  Enter 下一步  Esc 返回  q 退出",
            "←→ select  Enter next  Esc back  q quit",
        ),
        Step::ServiceMode => tr(
            lang,
            "←→ 选择  Enter 启动服务  Esc 返回  q 退出",
            "←→ select  Enter start  Esc back  q quit",
        ),
        Step::Done => tr(
            lang,
            "Enter 返回主菜单  Esc 返回  q 退出",
            "Enter back to menu  Esc back  q quit",
        ),
    };

    lay::render_hint_bar(frame, area, hint);
}

fn layout_detail(lang: UiLanguage, mode: LayoutMode) -> Vec<Line<'static>> {
    match mode {
        LayoutMode::TauriTavern => vec![
            Line::from(Span::styled("tauritavern", theme::title())),
            Line::from(""),
            Line::from(tr(
                lang,
                "适用：TauriTavern 的 data/ 目录结构。",
                "For: TauriTavern data/ layout.",
            )),
            Line::from(tr(
                lang,
                "workspace 建议指向：data/ 或 data/default-user/。",
                "workspace should point to: data/ or data/default-user/.",
            )),
            Line::from(""),
            Line::from("data/"),
            Line::from("  default-user/"),
            Line::from("  extensions/third-party/"),
            Line::from("  _tauritavern/"),
        ],
        LayoutMode::SillyTavern => vec![
            Line::from(Span::styled("sillytavern", theme::title())),
            Line::from(""),
            Line::from(tr(
                lang,
                "适用：SillyTavern 仓库布局（非 Docker）。",
                "For: SillyTavern repo layout (non-docker).",
            )),
            Line::from(tr(
                lang,
                "workspace 可指向：repo root / data / data/default-user/。",
                "workspace can be: repo root / data / data/default-user/.",
            )),
            Line::from(""),
            Line::from("repo/"),
            Line::from("  data/default-user/"),
            Line::from("  public/scripts/extensions/third-party/"),
        ],
        LayoutMode::SillyTavernDocker => vec![
            Line::from(Span::styled("sillytavern-docker", theme::title())),
            Line::from(""),
            Line::from(tr(
                lang,
                "适用：SillyTavern Docker 卷布局。",
                "For: SillyTavern docker volume layout.",
            )),
            Line::from(tr(
                lang,
                "workspace 可指向：docker root / data / data/default-user/。",
                "workspace can be: docker root / data / data/default-user/.",
            )),
            Line::from(""),
            Line::from("docker/"),
            Line::from("  data/default-user/"),
            Line::from("  extensions/"),
        ],
    }
}
