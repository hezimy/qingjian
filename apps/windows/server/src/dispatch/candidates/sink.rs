//! 候选窗口的输出端。

use qingjian_platform::protocol::{Frame, ScreenRect};
use qingjian_platform::{CandidateRenderer, FontSize};
use qingjian_render::Mode;

/// 拼音行（录入框）、候选字、译文三处的字号、字重与颜色覆盖（`[general]` 下的同名项）。
/// 字号 0 = 用主题的；字重 0 = 用主题的；颜色空 = 用主题的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextStyleSettings {
    /// 拼音行字号（`[general] input_font_size`）。
    pub input_size: FontSize,

    /// 候选字字号（`[general] candidate_font_size`）。
    pub candidate_size: FontSize,

    /// 译文字号（`[general] annotation_font_size`）。
    pub annotation_size: FontSize,

    /// 拼音行字重（`[general] input_weight`）。
    pub input_weight: u16,

    /// 候选字字重（`[general] candidate_weight`）。
    pub candidate_weight: u16,

    /// 译文字重（`[general] annotation_weight`）。
    pub annotation_weight: u16,

    /// 拼音行颜色（`[general] input_color`）。
    pub input_color: String,

    /// 候选字颜色（`[general] candidate_color`）。
    pub candidate_color: String,

    /// 译文颜色（`[general] annotation_color`）。
    pub annotation_color: String,
}

/// 候选窗口 / 状态条的画法。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderSettings {
    /// 由谁画（`[general] renderer`）。
    pub renderer: CandidateRenderer,

    /// 字族名（`[general] font`），空为系统字体。
    pub font: String,

    /// 主题 id（`[general] theme`，已按旧写法兼容解析）。
    pub theme: String,

    /// 拼音行 / 候选字 / 译文的字号、字重与颜色，盖过主题。
    pub text: TextStyleSettings,

    /// 候选列表高亮底色（`[general] highlight_color`）；空为用主题的高亮色。
    pub highlight_color: String,

    /// 候选窗口底色（`[general] surface_color`）；空为用主题的。
    pub surface_color: String,

    /// 生词译词是否用主题的醒目色标出（`[general] fresh_words`）。
    pub fresh_words: bool,

    /// 过渡动画（`[general] animations`）。
    pub animations: bool,

    /// 用户主题目录的戳（`ThemeLibrary::stamp`）：主题文件改了它就变，UI 线程据此重读主题。
    pub themes_stamp: u64,
}

/// Router 只产出帧，画交给它；Windows 上由 UI 线程实现。
pub trait CandidateSink: Send {
    /// 把候选窗口摆到 `rect`（组句范围的屏幕矩形）下方并按 `frame` 重绘；`mode` 给主题显示中 / 英等状态。
    fn show(&self, frame: Frame, mode: Mode, rect: ScreenRect);

    fn hide(&self);

    /// 换画法：装上时与配置热加载后调，只在设置变了时调。
    fn configure(&self, settings: RenderSettings);
}

/// 不画候选窗口的空实现。
pub struct NoopSink;

impl CandidateSink for NoopSink {
    fn show(&self, _frame: Frame, _mode: Mode, _rect: ScreenRect) {}

    fn hide(&self) {}

    fn configure(&self, _settings: RenderSettings) {}
}
