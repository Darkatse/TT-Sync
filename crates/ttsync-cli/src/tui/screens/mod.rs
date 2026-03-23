pub mod doctor;
pub mod help;
pub mod main_menu;
pub mod onboard;
pub mod pairing;
pub mod peers;
pub mod placeholder;
pub mod serve;

use crate::Context;
use crate::config::{self, UiLanguage};
use crate::tui::i18n::tr;

pub fn sync_folder_display(ctx: &Context, lang: UiLanguage) -> String {
    if !ctx.config_path.exists() {
        return tr(lang, "未指定", "unspecified").to_owned();
    }
    match config::load_config(&ctx.config_path) {
        Ok(cfg) => cfg.workspace_path.display().to_string(),
        Err(_) => tr(lang, "读取失败（运行 Doctor 查看）", "load failed (run Doctor)").to_owned(),
    }
}
