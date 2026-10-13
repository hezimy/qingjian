//! 设置里的文字覆盖：拼音行、候选字、译文三处各自的字号、字重与颜色。
//!
//! 候选 / 译文的字号盖过主题 `candidate`、`annotation` 两个文字样式，行高按同一比例缩放；
//! 序号（`index`）、拼音（`preedit`）与表格行高跟着候选字缩放，同一行才对得齐；主题自定义的其它样式不动。
//! 拼音行的字号是绝对值（它的节点用哪个文字样式由主题决定），字重与颜色同样按角色盖过。

use super::FontSpec;
use super::font::FontWeight;
use crate::color::Color;

/// 候选窗里的一种文字。设置里三处各记一份覆盖。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    /// 拼音行（正在输入的拼音）。
    Preedit,

    /// 候选字。
    Candidate,

    /// 译文。
    Annotation,
}

impl Role {
    /// 主题文字样式名对应的角色；其它样式（页脚等）不属于这三处。
    pub(crate) fn of_style(name: &str) -> Option<Self> {
        match name {
            "preedit" => Some(Self::Preedit),
            "candidate" | "index" => Some(Self::Candidate),
            "annotation" => Some(Self::Annotation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextSizes {
    /// 候选字字号（点）；`None` 用主题的。
    pub candidate: Option<f32>,

    /// 译文字号（点）；`None` 用主题的。
    pub annotation: Option<f32>,

    /// 拼音行字号（点）；`None` 用主题的。绝对值，不跟候选字缩放。
    pub preedit: Option<f32>,

    /// 候选字字重；`None` 用主题的。
    pub candidate_weight: Option<FontWeight>,

    /// 译文字重；`None` 用主题的。
    pub annotation_weight: Option<FontWeight>,

    /// 拼音行字重；`None` 用主题的。
    pub preedit_weight: Option<FontWeight>,

    /// 候选字颜色；`None` 用主题的。
    pub candidate_color: Option<Color>,

    /// 译文颜色；`None` 用主题的。
    pub annotation_color: Option<Color>,

    /// 拼音行颜色；`None` 用主题的。
    pub preedit_color: Option<Color>,
}

impl TextSizes {
    /// 配置里的数：不是正数的当没填。字重与颜色留空（用主题的）。
    pub fn new(candidate: f32, annotation: f32) -> Self {
        let positive = |size: f32| (size.is_finite() && size > 0.0).then_some(size);
        Self {
            candidate: positive(candidate),
            annotation: positive(annotation),
            ..Self::default()
        }
    }

    /// 这一处设置的字重；没填返回 `None`。
    pub(crate) fn weight(&self, role: Role) -> Option<FontWeight> {
        match role {
            Role::Preedit => self.preedit_weight,
            Role::Candidate => self.candidate_weight,
            Role::Annotation => self.annotation_weight,
        }
    }

    /// 这一处设置的颜色；没填返回 `None`。
    pub(crate) fn color(&self, role: Role) -> Option<Color> {
        match role {
            Role::Preedit => self.preedit_color,
            Role::Candidate => self.candidate_color,
            Role::Annotation => self.annotation_color,
        }
    }

    /// 这一处设置的绝对字号；没填返回 `None`。
    pub(crate) fn size(&self, role: Role) -> Option<f32> {
        match role {
            Role::Preedit => self.preedit,
            Role::Candidate => self.candidate,
            Role::Annotation => self.annotation,
        }
    }

    /// `name` 样式相对主题的缩放比；`theme_size` 是主题里对应基准样式（候选或译文）的字号。
    pub(super) fn ratio(&self, name: &str, theme_size: impl Fn(&str) -> Option<f32>) -> f32 {
        let (wanted, base) = match name {
            "candidate" | "index" | "preedit" => (self.candidate, "candidate"),
            "annotation" => (self.annotation, "annotation"),
            _ => return 1.0,
        };
        match (wanted, theme_size(base)) {
            (Some(wanted), Some(base)) if base > 0.0 => wanted / base,
            _ => 1.0,
        }
    }
}

/// 字号与行高一起乘。
pub(super) fn scaled(spec: FontSpec, ratio: f32) -> FontSpec {
    FontSpec {
        size: spec.size * ratio,
        line_height: spec.line_height * ratio,
        ..spec
    }
}

/// 用设置里的绝对字号与字重盖过 `spec`；没填的那项不动。行高按字号同比例缩放。
pub(super) fn overridden(
    spec: FontSpec,
    size: Option<f32>,
    weight: Option<FontWeight>,
) -> FontSpec {
    let spec = match size {
        Some(size) if spec.size > 0.0 => FontSpec {
            size,
            line_height: spec.line_height * size / spec.size,
            ..spec
        },
        _ => spec,
    };
    match weight {
        Some(weight) => FontSpec { weight, ..spec },
        None => spec,
    }
}
