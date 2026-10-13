//! 候选窗口的一行：序号、候选词、annotation 片段。

use super::tone::Tone;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 显示用序号文本，如 `1`。
    pub index: String,

    /// 候选词。
    pub text: String,

    /// 紧跟在候选词后面的辅码，如 `[kf]`：码是词本身的属性，不进右侧的 annotation 列。
    pub code: Option<String>,

    /// 右侧 annotation，按顺序绘制；没有译文时为空。
    pub annotation: Vec<(String, Tone)>,

    /// 第二种学习语言第一条译词在 `annotation` 里的下标（前面是第一种语言与「英 / 日 / 西」标签）。
    /// 只有一种语言或没开第二种时为 `None`。点译词上屏时按它分语言。
    pub alt_gloss_from: Option<usize>,

    /// 来自云联想：词前画一个小云朵，与本地候选区分。
    pub cloud: bool,
}

impl Row {
    /// 只有序号和候选词的一行。
    pub fn plain(index: usize, text: impl Into<String>) -> Self {
        Self {
            index: (index + 1).to_string(),
            text: text.into(),
            code: None,
            annotation: Vec::new(),
            alt_gloss_from: None,
            cloud: false,
        }
    }
}
