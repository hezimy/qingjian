//! 字体库：不扫系统字体目录（fontdb 全扫几百毫秒、几十 MB），按平台清单只加载界面字体、中文、日文、emoji 几个文件。
//!
//! 文件走 fontdb 的 mmap 加载，只解析名字表与 cmap，Apple Color Emoji 那种 190 MB 的文件也只在用到字形时才读页。
//! 中日同形字按 locale 回退（cosmic-text 的平台回退表：zh-CN → PingFang SC，ja → Hiragino Sans）。

#[cfg(target_os = "windows")]
pub mod directwrite;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod trak;
mod ui_font;
#[cfg(target_os = "windows")]
mod windows;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use cosmic_text::fontdb::{Database, Family, Source};
use cosmic_text::{Fallback, FontSystem, PlatformFallback};
use unicode_script::Script;

use crate::error::RenderError;

pub use trak::Trak;
pub use ui_font::UiFont;

#[cfg(target_os = "windows")]
use self::windows as platform;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;

pub struct FontLibrary {
    /// 已加载的字体。
    db: Database,

    /// 界面字体的字族名（`SansSerif` 映射到它）。
    ui_family: String,

    /// 中日同形字回退用的 locale，如 `zh-CN`。
    locale: String,

    /// 逐字形兜底里要排除的字族，见 [`fallback_only_families`]。
    forbidden: Vec<&'static str>,
}

impl FontLibrary {
    /// 按平台清单加载系统字体。`locale` 决定 Han 字形选哪家（`zh-CN` / `zh-TW` / `ja`）。
    pub fn system(locale: &str) -> Result<Self, RenderError> {
        Self::build(locale, None)
    }

    /// 界面字体换成用户指定的字族，系统字体仍加载在后面当回退；指定的字体一个文件都没加载到或名字对不上就退回系统字体。
    pub fn with_ui_font(locale: &str, ui_font: &UiFont) -> Result<Self, RenderError> {
        Self::build(locale, Some(ui_font))
    }

    fn build(locale: &str, ui_font: Option<&UiFont>) -> Result<Self, RenderError> {
        let mut db = Database::new();
        let custom_family = ui_font.and_then(|font| {
            let loaded = font.files.iter().filter(|path| load(&mut db, path)).count();
            let found = db.faces().any(|face| {
                face.families
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(&font.family))
            });
            if loaded > 0 && found {
                Some(font.family.clone())
            } else {
                tracing::warn!(
                    family = font.family,
                    files = font.files.len(),
                    loaded,
                    "指定的界面字体没找到，用系统字体"
                );
                None
            }
        });
        let ui =
            load_first(&mut db, &platform::ui_fonts()).ok_or_else(|| RenderError::NoUiFont {
                tried: platform::ui_fonts(),
            })?;
        let ui_family = custom_family.unwrap_or_else(|| {
            db.face(ui)
                .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
                .unwrap_or_else(|| "sans-serif".to_owned())
        });
        db.set_sans_serif_family(ui_family.clone());
        for path in platform::script_fonts(locale) {
            if drawable(&path) {
                load(&mut db, &path);
            } else {
                tracing::debug!(path = %path.display(), "字体不存在，或没有 swash 能画的轮廓表，跳过");
            }
        }
        for path in platform::ui_weight_fonts()
            .into_iter()
            .chain(platform::emoji_fonts())
        {
            if !load(&mut db, &path) {
                tracing::debug!(path = %path.display(), "字体文件不存在，跳过");
            }
        }
        let forbidden = fallback_only_families(&db, &platform::fallback_only_fonts(locale));
        tracing::debug!(faces = db.len(), ui_family, forbidden = ?forbidden, "字体库就绪");
        Ok(Self {
            db,
            ui_family,
            locale: locale.to_owned(),
            forbidden,
        })
    }

    /// 已加载的字族名，按加载顺序去重。
    pub fn families(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for face in self.db.faces() {
            for (name, _) in &face.families {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names
    }

    /// 已加载的字体文件，按加载顺序去重。快照测试拿它判断字体环境变没变。
    pub fn font_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        for face in self.db.faces() {
            let path = match &face.source {
                Source::File(path) | Source::SharedFile(path, _) => path,
                Source::Binary(_) => continue,
            };
            if !files.contains(path) {
                files.push(path.clone());
            }
        }
        files
    }

    pub fn ui_family(&self) -> &str {
        &self.ui_family
    }

    /// 交给 cosmic-text。
    pub fn into_font_system(self) -> FontSystem {
        let mut db = self.db;
        db.set_sans_serif_family(self.ui_family);
        FontSystem::new_with_locale_and_db_and_fallback(
            self.locale,
            db,
            LibraryFallback {
                forbidden: self.forbidden,
            },
        )
    }
}

/// 平台自带的回退表，外加一份「逐字形兜底里禁用」的字族。
///
/// cosmic-text 的逐字形回退分三段：脚本表（Han → 微软雅黑之类，要求字重**精确**命中）、通用表（Segoe UI 等），
/// 最后一段按字重差距把库里任何有字形的字体兜一遍。汉字字体只有 300 / 400 / 700 三档，字重写 500 这类中间值时
/// 脚本表过不去，兜底就会挑中恰好有该字重的日文字体（Windows 上 Yu Gothic 有 Medium），把「直 / 骨 / 曜」
/// 这些汉日同形字画成日文写法、笔画也跟着变毛。
///
/// 这里把「只为回退加载的外文字体」列进禁用表——`forbidden_fallback` 只作用于最后那段兜底，脚本表照旧，
/// 所以假名仍然拿得到 Yu Gothic，汉字则只能落到汉字字体上。
struct LibraryFallback {
    /// 禁用的字族名，见 [`fallback_only_families`]。
    forbidden: Vec<&'static str>,
}

impl Fallback for LibraryFallback {
    fn common_fallback(&self) -> &[&'static str] {
        PlatformFallback.common_fallback()
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        &self.forbidden
    }

    fn script_fallback(&self, script: Script, locale: &str) -> &[&'static str] {
        PlatformFallback.script_fallback(script, locale)
    }
}

/// 这些字体文件里的字族名，进 [`LibraryFallback`] 的禁用表；文件没加载进 `db` 就没有名字。
///
/// 回退表存的是 `&'static str`，所以名字要 leak；字族名只有几个、一次进程内复用同一份，不重复 leak。
fn fallback_only_families(db: &Database, paths: &[PathBuf]) -> Vec<&'static str> {
    static INTERNED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut interned = INTERNED.lock().unwrap_or_else(|error| error.into_inner());
    let mut names: Vec<&'static str> = Vec::new();
    for face in db.faces() {
        let (Source::File(path) | Source::SharedFile(path, _)) = &face.source else {
            continue;
        };
        if !paths.iter().any(|want| want == path) {
            continue;
        }
        for (name, _) in &face.families {
            let known = interned
                .iter()
                .find(|have| have.eq_ignore_ascii_case(name))
                .copied()
                .unwrap_or_else(|| {
                    let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
                    interned.push(leaked);
                    leaked
                });
            if !names.contains(&known) {
                names.push(known);
            }
        }
    }
    names
}

/// 候选窗里的文字能真正渲染出来的字重档位（升序），设置页的字重下拉按它列项。
///
/// `font_files` 是用户指定的候选窗字体的文件，空为系统字体。
///
/// 系统字体下按**汉字字体**（Windows 是微软雅黑：Light 290 / Regular 400 / Bold 700）列：候选字与译文主要是汉字，
/// 汉字字体没有的档位虽然也画得出来，但汉字会被逐字形回退换成别的字体（见 [`LibraryFallback`]），看着就不对了。
/// 界面字体没有对应档位时（如 290）会取它最近的档位，字形仍然正常。
pub fn candidate_weights(locale: &str, font_files: &[PathBuf]) -> Vec<u16> {
    if !font_files.is_empty() {
        return files_weights(font_files);
    }
    let han = files_weights(&platform::han_fonts(locale));
    if !han.is_empty() {
        return han;
    }
    // 没有汉字字体（漏装 / 路径变了）时退回界面字体的档位，别给个空下拉
    let mut ui: Vec<PathBuf> = platform::ui_fonts()
        .into_iter()
        .find(|path| !file_weights(path).is_empty())
        .into_iter()
        .collect();
    ui.extend(platform::ui_weight_fonts());
    files_weights(&ui)
}

/// 一组字体文件里声明过的字重并集（升序）。
fn files_weights(paths: &[PathBuf]) -> Vec<u16> {
    let mut weights: Vec<u16> = paths.iter().flat_map(|path| file_weights(path)).collect();
    weights.sort_unstable();
    weights.dedup();
    weights
}

/// 一个字体文件里各面声明过的字重（升序）；文件不存在或解析不了返回空。
fn file_weights(path: &Path) -> Vec<u16> {
    let mut db = Database::new();
    if db.load_font_file(path).is_err() {
        return Vec::new();
    }
    let mut weights: Vec<u16> = db.faces().map(|face| face.weight.0).collect();
    weights.sort_unstable();
    weights.dedup();
    weights
}

/// 界面字体用的字族。
pub const UI_FAMILY: Family<'static> = Family::SansSerif;

/// 依次尝试，加载成功的第一个文件的第一张面。
fn load_first(db: &mut Database, paths: &[PathBuf]) -> Option<cosmic_text::fontdb::ID> {
    paths.iter().find_map(|path| {
        let before: Vec<_> = db.faces().map(|f| f.id).collect();
        load(db, path)
            .then(|| db.faces().map(|f| f.id).find(|id| !before.contains(id)))
            .flatten()
    })
}

/// 文件存在且能解析就加载；返回是否加载了。
fn load(db: &mut Database, path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    match db.load_font_file(path) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "字体文件解析失败");
            false
        }
    }
}

/// 文件里至少有一张面带 swash 能栅格化的轮廓（`glyf` / `CFF `）。先放进一个临时字体库看（mmap，只解析表头），
/// 正式的库里只加能画的：macOS 26 起的苹方把轮廓放进私有的 `hvgl` 表，swash 读不出字形，文字会画成空白，跳过它回退才会落到下一个字体。
fn drawable(path: &Path) -> bool {
    let mut probe = Database::new();
    if !load(&mut probe, path) {
        return false;
    }
    let ids: Vec<_> = probe.faces().map(|face| face.id).collect();
    ids.into_iter()
        .any(|id| probe.with_face_data(id, has_outline_table).unwrap_or(false))
}

fn has_outline_table(data: &[u8], index: u32) -> bool {
    let Ok(face) = ttf_parser::RawFace::parse(data, index) else {
        return false;
    };
    [b"glyf", b"CFF "]
        .into_iter()
        .any(|tag| face.table(ttf_parser::Tag::from_bytes(tag)).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 设置里的字重档位：升序去重，系统字体下按汉字字体的档位列（Windows 上是微软雅黑的三档，
    /// 不是界面字体与它的交集——290 这种档位界面字体没有，但汉字画得好好的）。
    #[test]
    fn candidate_weights_are_sorted_and_follow_the_han_font() {
        let weights = candidate_weights("zh-CN", &[]);
        assert!(
            weights.windows(2).all(|pair| pair[0] < pair[1]),
            "{weights:?}"
        );
        let han = files_weights(&platform::han_fonts("zh-CN"));
        if !han.is_empty() {
            assert_eq!(weights, han);
        }
    }

    /// 指定了候选窗字体就按它自己的档位列。
    #[test]
    fn candidate_weights_of_a_custom_font_come_from_its_files() {
        let Some(path) = platform::ui_fonts().into_iter().find(|path| path.is_file()) else {
            return;
        };
        let files = std::slice::from_ref(&path);
        assert_eq!(candidate_weights("zh-CN", files), file_weights(&path));
    }
}
