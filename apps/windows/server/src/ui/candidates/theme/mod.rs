//! 候选窗口主题：字体、颜色、间距。视觉层级对齐 macOS 端；GDI 没有随外观切换的语义色，浅 / 深各写一套（[`Palette`]）。
//!
//! 系统绘制不经过青简渲染器，设置里的字体、字号、字重、颜色与生词开关由 [`set_look`] 下发到这里，
//! 没设过的项用原来的默认（微软雅黑 UI，候选 16 / 译文 12 / 序号 11）。

mod palette;

use std::sync::Mutex;

use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DeleteObject,
    FF_DONTCARE, HFONT, OUT_TT_PRECIS, VARIABLE_PITCH,
};
use windows::core::{PCWSTR, w};

use crate::dispatch::RenderSettings;

use self::palette::Palette;

/// 常规字重；windows crate 未导出。
const FW_NORMAL: u16 = 400;

/// 没选字体时用的字族，与青简渲染器的界面字体一致。
const DEFAULT_FACE: &str = "Microsoft YaHei UI";

/// 默认字号（点）：候选字、译文、序号、状态条的符号。
const DEFAULT_CANDIDATE: f32 = 16.0;
const DEFAULT_ANNOTATION: f32 = 12.0;
const DEFAULT_INDEX: f32 = 11.0;
const DEFAULT_SYMBOL: f32 = 15.0;

/// COLORREF 低位到高位是 R、G、B。
pub(super) const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

/// 设置里给系统绘制的那一份外观（`[general]` 下的字体、字号、字重、颜色、生词开关）。
/// 没设过的项为 `None`，用上面的默认值。
#[derive(Debug, Clone)]
pub(crate) struct Look {
    /// 字族名（`[general] font`）；空为微软雅黑 UI，没装的字体由 GDI 自己回落。
    pub(crate) family: String,

    /// 候选字字号（点）。
    pub(crate) candidate_size: Option<f32>,

    /// 译文字号（点）。
    pub(crate) annotation_size: Option<f32>,

    /// 拼音行字号（点）；没设过跟译文一样。
    pub(crate) input_size: Option<f32>,

    pub(crate) candidate_weight: Option<u16>,
    pub(crate) annotation_weight: Option<u16>,
    pub(crate) input_weight: Option<u16>,

    pub(crate) candidate_color: Option<COLORREF>,
    pub(crate) annotation_color: Option<COLORREF>,
    pub(crate) input_color: Option<COLORREF>,

    /// 当前候选那一行的底色。
    pub(crate) highlight: Option<COLORREF>,

    /// 窗口底色。
    pub(crate) surface: Option<COLORREF>,

    /// 生词译词用醒目色标出。
    pub(crate) fresh_words: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            family: String::new(),
            candidate_size: None,
            annotation_size: None,
            input_size: None,
            candidate_weight: None,
            annotation_weight: None,
            input_weight: None,
            candidate_color: None,
            annotation_color: None,
            input_color: None,
            highlight: None,
            surface: None,
            fresh_words: true,
        }
    }
}

/// 配置下发过来的外观。UI 线程单线程用，加把锁只是为了让静态量过编译期检查。
static LOOK: Mutex<Option<Look>> = Mutex::new(None);

/// UI 线程收到配置时调（`UiCommand::Configure`）；没收到过就是默认那一套。
pub(crate) fn set_look(look: Look) {
    if let Ok(mut guard) = LOOK.lock() {
        *guard = Some(look);
    }
}

fn look() -> Look {
    LOOK.lock()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_default()
}

impl Look {
    /// 设置 → 系统绘制那一套。颜色写得不对、字号不是正数的项当没填（用默认）。
    pub(crate) fn from_settings(settings: &RenderSettings) -> Self {
        let text = &settings.text;
        Self {
            family: settings.font.clone(),
            candidate_size: text.candidate_size.get(),
            annotation_size: text.annotation_size.get(),
            input_size: text.input_size.get(),
            candidate_weight: weight(text.candidate_weight),
            annotation_weight: weight(text.annotation_weight),
            input_weight: weight(text.input_weight),
            candidate_color: color(&text.candidate_color),
            annotation_color: color(&text.annotation_color),
            input_color: color(&text.input_color),
            highlight: color(&settings.highlight_color),
            surface: color(&settings.surface_color),
            fresh_words: settings.fresh_words,
        }
    }
}

/// 配置里的字重：0 = 用默认的，其余夹到 100–900。
fn weight(value: u16) -> Option<u16> {
    (value > 0).then(|| value.clamp(100, 900))
}

/// `#RRGGBB` 或 `RRGGBB`；写得不对当没填。
fn color(text: &str) -> Option<COLORREF> {
    let text = text.trim().trim_start_matches('#');
    if text.len() != 6 || !text.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |range: std::ops::Range<usize>| u8::from_str_radix(&text[range], 16).ok();
    Some(rgb(channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

/// 一套配色 + 按 DPI 造好的字体。字体是 GDI 资源，`Drop` 里删。
pub(crate) struct Theme {
    pub text_font: HFONT,

    pub annotation_font: HFONT,

    pub index_font: HFONT,

    /// 拼音行的字体：设置里没单给拼音行字号时就是译文的字号。
    pub preedit_font: HFONT,

    /// 符号字体（状态条的齿轮 ⚙）：雅黑没有这些字形。
    pub symbol_font: HFONT,

    pub text_color: COLORREF,

    pub gloss_color: COLORREF,

    pub pos_color: COLORREF,

    /// 生词译文，比普通译文醒目。
    pub fresh_color: COLORREF,

    pub index_color: COLORREF,

    /// 云联想的云朵与文字。
    pub cloud_color: COLORREF,

    /// 拼音行里正在敲的那一段（设置里没给颜色时与普通译文同色）。
    pub preedit_color: COLORREF,

    pub background: COLORREF,

    /// 当前候选的高亮底色（mac 的半透明蓝预混成不透明值，GDI 无 alpha）。
    pub highlight: COLORREF,

    /// 生词译词是否用 `fresh_color` 标出。
    pub fresh_words: bool,

    /// 窗口内边距（已按 DPI 缩放）。
    pub padding: i32,

    /// 行内上下留白。
    pub row_padding: i32,

    /// 列间距。
    pub column_gap: i32,

    /// 窗口圆角半径。
    pub corner_radius: i32,
}

impl Theme {
    /// `dpi` 96 为 100%。字体、字号、字重、颜色与生词开关取设置下发的那一份；
    /// 设置里没给过的项用原来的默认（微软雅黑 UI，候选 16 / 译文 12 / 序号 11）。
    pub(crate) fn new(dpi: u32, dark: bool) -> Self {
        let look = look();
        let scale = |points: f32| (points * dpi as f32 / 96.0).round().max(1.0) as i32;
        let size = |wanted: Option<f32>, fallback: f32| scale(wanted.unwrap_or(fallback));
        let candidate = size(look.candidate_size, DEFAULT_CANDIDATE);
        let annotation = size(look.annotation_size, DEFAULT_ANNOTATION);
        // 序号跟着候选字同比例，同一行才对得齐（青简渲染器也是这样）。
        let ratio = look
            .candidate_size
            .filter(|size| size.is_finite() && *size > 0.0)
            .map_or(1.0, |size| size / DEFAULT_CANDIDATE);
        // 负高度 = 字符高度（不含内部行距）。
        let face = wide(if look.family.trim().is_empty() {
            DEFAULT_FACE
        } else {
            look.family.trim()
        });
        let font = |height: i32, weight: u16| create_font(-height, weight, PCWSTR(face.as_ptr()));
        let palette = if dark {
            Palette::dark()
        } else {
            Palette::light()
        };
        Self {
            text_font: font(candidate, look.candidate_weight.unwrap_or(FW_NORMAL)),
            annotation_font: font(annotation, look.annotation_weight.unwrap_or(FW_NORMAL)),
            index_font: font(scale(DEFAULT_INDEX * ratio), FW_NORMAL),
            // 拼音行没单独给字号时用译文的（青简渲染器里拼音行引用的就是 annotation 样式）
            preedit_font: font(
                size(
                    look.input_size,
                    look.annotation_size.unwrap_or(DEFAULT_ANNOTATION),
                ),
                look.input_weight.unwrap_or(FW_NORMAL),
            ),
            symbol_font: create_font(-scale(DEFAULT_SYMBOL), FW_NORMAL, w!("Segoe UI Symbol")),
            text_color: look.candidate_color.unwrap_or(palette.text_color),
            gloss_color: look.annotation_color.unwrap_or(palette.gloss_color),
            pos_color: palette.pos_color,
            fresh_color: palette.fresh_color,
            index_color: palette.index_color,
            cloud_color: palette.cloud_color,
            preedit_color: look.input_color.unwrap_or(palette.gloss_color),
            background: look.surface.unwrap_or(palette.background),
            highlight: look.highlight.unwrap_or(palette.highlight),
            fresh_words: look.fresh_words,
            padding: scale(8.0),
            row_padding: scale(4.0),
            column_gap: scale(8.0),
            corner_radius: scale(8.0),
        }
    }
}

impl Drop for Theme {
    fn drop(&mut self) {
        for font in [
            self.text_font,
            self.annotation_font,
            self.index_font,
            self.preedit_font,
            self.symbol_font,
        ] {
            if !font.is_invalid() {
                let _ = unsafe { DeleteObject(font.into()) };
            }
        }
    }
}

/// 字族名的宽字符写法（含结尾 NUL）；`CreateFontW` 会复制它。
fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 缺字由 GDI 字体链回落。`height` 为负的字符高度。
fn create_font(height: i32, weight: u16, face: PCWSTR) -> HFONT {
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            i32::from(weight),
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (VARIABLE_PITCH.0 | FF_DONTCARE.0) as u32,
            face,
        )
    }
}
