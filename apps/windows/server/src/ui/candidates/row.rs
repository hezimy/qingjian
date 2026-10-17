//! 候选窗口的一行：[`Candidate`] → 渲染器的 [`Row`]（序号、候选词、annotation 片段），与 macOS 端 `candidates/row.rs` 一致。
//! GDI 画法也用同一个类型。

use qingjian_core::{Candidate, CandidateKind, Language, Translation};
use qingjian_render::{Row, Tone};

/// `position` 是页内下标（从 0 起）。`show_code` 是 `[general] aux_code_show`：
/// 打开且候选带码时，码用方括号括起来紧跟在候选词后面（`鹤[rbm]`），不进 annotation。
/// `show_pos` 是 `[general] translation_pos`：关掉时译词保留、词性段（v. / n. / adj.）不画。
pub(crate) fn from_candidate(
    position: usize,
    candidate: &Candidate,
    show_code: bool,
    show_pos: bool,
) -> Row {
    let code = candidate
        .aux_code
        .as_ref()
        .filter(|_| show_code)
        .map(|code| format!("[{code}]"));
    let mut annotation = Vec::new();
    if let Some(reading) = &candidate.reading {
        annotation.push((reading.clone(), Tone::Gloss));
    }
    // 两种语言同时开着时给每串译词前面加「英 / 日 / 西」标签，好在候选旁分清哪串是哪门语言
    let both = candidate.translation.is_some() && candidate.alt_translation.is_some();
    if let Some(translation) = &candidate.translation {
        if both {
            push_label(&mut annotation, translation.language);
            push_translation(&mut annotation, translation, false, show_pos);
        } else {
            let leading = !annotation.is_empty();
            push_translation(&mut annotation, translation, leading, show_pos);
        }
    }
    // 第二种语言第一条译词的下标：前面的分隔符与标签算第一种语言，点译词按它分语言
    let alt_gloss_from = candidate.alt_translation.as_ref().map(|translation| {
        if both {
            push_label(&mut annotation, translation.language);
        } else if !annotation.is_empty() {
            // 配置上不该出现（只填第二种时不装第二种表），仍按单语言那样补个分隔符
            annotation.push((" · ".to_owned(), Tone::Separator));
        }
        annotation.len()
    });
    if let Some(translation) = &candidate.alt_translation {
        push_translation(&mut annotation, translation, false, show_pos);
    }
    Row {
        index: (position + 1).to_string(),
        text: candidate.text.clone(),
        code,
        annotation,
        alt_gloss_from,
        cloud: candidate.kind == CandidateKind::Cloud,
    }
}

/// 「英 / 日 / 西 / 越 / 德」：两种语言同时显示时给每串译词标出来源。
fn language_mark(language: Language) -> &'static str {
    match language {
        Language::English => "英",
        Language::Japanese => "日",
        Language::Spanish => "西",
        Language::Vietnamese => "越",
        Language::German => "德",
        Language::Chinese => "中",
    }
}

/// 给一串译词前面加「英 / 日 / 西 / 越 / 德」标签；前面已有内容时先补一个分隔符。
fn push_label(annotation: &mut Vec<(String, Tone)>, language: Language) {
    if !annotation.is_empty() {
        annotation.push((" · ".to_owned(), Tone::Separator));
    }
    annotation.push((format!("{} ", language_mark(language)), Tone::Faint));
}

/// 把一门语言的译词接到 annotation 后面。`leading_separator` 为真时（前面已有读音）先补一个分隔符；
/// `show_pos` 为假时（`[general] translation_pos` 关掉）不画词性段。
fn push_translation(
    annotation: &mut Vec<(String, Tone)>,
    translation: &Translation,
    leading_separator: bool,
    show_pos: bool,
) {
    for (i, sense) in translation.senses().iter().enumerate() {
        if i > 0 || leading_separator {
            annotation.push((" · ".to_owned(), Tone::Separator));
        }
        if show_pos && let Some(pos) = sense.part_of_speech {
            annotation.push((format!("{pos} "), Tone::Pos));
        }
        let tone = if sense.fresh {
            Tone::Fresh
        } else {
            Tone::Gloss
        };
        for segment in sense.furigana() {
            annotation.push((segment.text, tone));
            if let Some(reading) = segment.reading {
                annotation.push((format!("({reading})"), Tone::Faint));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use qingjian_core::{Language, PartOfSpeech, Sense, Translation};

    use super::*;

    fn candidate_with_gloss() -> Candidate {
        Candidate {
            text: "开发".to_owned(),
            kind: CandidateKind::Chinese,
            syllables: vec!["kai".to_owned(), "fa".to_owned()],
            reading: None,
            translation: Some(Translation::new(
                Language::English,
                vec![Sense {
                    part_of_speech: Some(PartOfSpeech::Verb),
                    text: "develop".to_owned(),
                    reading: None,
                    fresh: false,
                }],
            )),
            aux_code: None,
            alt_translation: None,
        }
    }

    #[test]
    fn part_of_speech_follows_the_frame_switch() {
        let on = from_candidate(0, &candidate_with_gloss(), false, true);
        let joined: String = on
            .annotation
            .iter()
            .map(|(text, _)| text.as_str())
            .collect();
        assert!(joined.contains("v. "), "{joined}");
        assert!(joined.contains("develop"), "{joined}");

        // 关掉后词性段消失，译词与分隔结构原样保留
        let off = from_candidate(0, &candidate_with_gloss(), false, false);
        let joined: String = off
            .annotation
            .iter()
            .map(|(text, _)| text.as_str())
            .collect();
        assert!(!joined.contains("v. "), "{joined}");
        assert!(joined.contains("develop"), "{joined}");
        assert_eq!(on.annotation.len() - 1, off.annotation.len());
    }

    #[test]
    fn a_gloss_without_part_of_speech_is_unchanged_by_the_switch() {
        let mut candidate = candidate_with_gloss();
        candidate.translation.as_mut().unwrap().senses_mut()[0].part_of_speech = None;
        for show_pos in [true, false] {
            let row = from_candidate(0, &candidate, false, show_pos);
            let joined: String = row
                .annotation
                .iter()
                .map(|(text, _)| text.as_str())
                .collect();
            assert_eq!(joined, "develop");
        }
    }
}
