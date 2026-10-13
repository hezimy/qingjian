//! Windows 的字体文件清单：Segoe UI、微软雅黑、Yu Gothic、Segoe UI Emoji（COLRv0）。

use std::path::PathBuf;

fn fonts_dir() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts")
}

pub(super) fn ui_fonts() -> Vec<PathBuf> {
    vec![
        fonts_dir().join("segoeui.ttf"),
        fonts_dir().join("arial.ttf"),
    ]
}

/// Segoe UI 的其余字重（主题里写 `weight` 时用）：细、半细、半粗、粗、特粗各一个文件。
pub(super) fn ui_weight_fonts() -> Vec<PathBuf> {
    let dir = fonts_dir();
    [
        "segoeuil.ttf",
        "segoeuisl.ttf",
        "seguisb.ttf",
        "segoeuib.ttf",
        "seguibl.ttf",
    ]
    .into_iter()
    .map(|file| dir.join(file))
    .collect()
}

/// 汉字字体（常规、粗、细三个文件）；日文环境下 Yu Gothic 自己就是汉字字体。
pub(super) fn han_fonts(locale: &str) -> Vec<PathBuf> {
    let dir = fonts_dir();
    if locale.starts_with("zh-TW") || locale.starts_with("zh-HK") {
        ["msjh.ttc", "msjhbd.ttc", "msjhl.ttc"]
            .map(|file| dir.join(file))
            .to_vec()
    } else if locale.starts_with("ja") {
        japanese_fonts()
    } else {
        ["msyh.ttc", "msyhbd.ttc", "msyhl.ttc"]
            .map(|file| dir.join(file))
            .to_vec()
    }
}

/// 日文字体（常规、Medium、粗、细四个文件）。
pub(super) fn japanese_fonts() -> Vec<PathBuf> {
    let dir = fonts_dir();
    ["YuGothR.ttc", "yugothm.ttc", "YuGothB.ttc", "YuGothL.ttc"]
        .into_iter()
        .map(|file| dir.join(file))
        .collect()
}

/// 汉字字体加日文回退：假名（日语译文的假名部分）靠后者。
pub(super) fn script_fonts(locale: &str) -> Vec<PathBuf> {
    let mut fonts = han_fonts(locale);
    if !locale.starts_with("ja") {
        fonts.extend(japanese_fonts());
    }
    fonts
}

/// 只为回退加载、不该在逐字形兜底里抢汉字的字体，见 `fonts::fallback_only_families`。
/// 日文环境里 Yu Gothic 是汉字字体本身，不能禁。
pub(super) fn fallback_only_fonts(locale: &str) -> Vec<PathBuf> {
    if locale.starts_with("ja") {
        Vec::new()
    } else {
        japanese_fonts()
    }
}

pub(super) fn emoji_fonts() -> Vec<PathBuf> {
    vec![fonts_dir().join("seguiemj.ttf")]
}
