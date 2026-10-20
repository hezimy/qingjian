//! 输入框的编号：编辑状态按它存在 `Settings::fields` 里。

/// 输入框：云服务页的三个，外观页的五个颜色，短语编辑卡片里的两个，生词本页的搜索框。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldId {
    BaseUrl,
    Model,
    ApiKey,
    /// 外观页的候选字颜色。
    CandidateColor,
    /// 外观页的译文颜色。
    AnnotationColor,
    /// 外观页的拼音行颜色。
    InputColor,
    /// 外观页的窗口背景色。
    SurfaceColor,
    /// 外观页的高亮颜色。
    HighlightColor,
    PhraseCode,
    PhraseText,
    /// 生词本页的搜索框，不存进配置。
    VocabularySearch,
}

impl FieldId {
    /// 存进配置的那几个（短语的两个随卡片一起存）。
    pub const ALL: [Self; 8] = [
        Self::BaseUrl,
        Self::Model,
        Self::ApiKey,
        Self::CandidateColor,
        Self::AnnotationColor,
        Self::InputColor,
        Self::SurfaceColor,
        Self::HighlightColor,
    ];

    /// 配置文件里的键：服务在 `[predict]`，颜色在 `[general]`。
    pub fn key(self) -> &'static str {
        match self {
            Self::BaseUrl => "base_url",
            Self::Model => "model",
            Self::ApiKey => "api_key",
            Self::CandidateColor => "candidate_color",
            Self::AnnotationColor => "annotation_color",
            Self::InputColor => "input_color",
            Self::SurfaceColor => "surface_color",
            Self::HighlightColor => "highlight_color",
            Self::PhraseCode => "code",
            Self::PhraseText => "text",
            Self::VocabularySearch => "vocabulary-search",
        }
    }
}
