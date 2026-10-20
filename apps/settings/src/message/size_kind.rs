//! 外观页的字号步进框：候选字号、译文字号、序号字号、拼音行字号。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeKind {
    Candidate,
    Annotation,
    Index,
    /// 拼音行（录入框）。
    Input,
}
