use crate::config::UiLanguage;

/// Pick the correct string for the current language.
pub fn tr(lang: UiLanguage, zh: &'static str, en: &'static str) -> &'static str {
    match lang {
        UiLanguage::ZhCn => zh,
        UiLanguage::En => en,
    }
}
