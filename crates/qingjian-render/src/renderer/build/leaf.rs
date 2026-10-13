//! 叶子节点：文字、图标、拼音行、译文。绑定取不到数据时不产出节点。

use taffy::{Dimension, NodeId, Style};

use super::layout_style;
use super::{Builder, Context};
use crate::error::RenderError;
use crate::frame::{Preedit, PreeditStyle, Tone};
use crate::scene::Visual;
use crate::theme::Role;
use crate::theme::file::node::{BoxSpec, Direction, NodeKind, NodeSpec, ToneFilter};

impl Builder<'_> {
    pub(super) fn leaf(
        &mut self,
        spec: &NodeSpec,
        layout: &BoxSpec,
        ctx: Context,
    ) -> Result<Option<NodeId>, RenderError> {
        let style = layout_style::from_box(layout, self.scale);
        let node = match &spec.kind {
            NodeKind::Text {
                bind,
                text,
                font,
                color,
                stroke,
            } => {
                let content = match (bind, text) {
                    (Some(bind), _) => ctx.text(bind),
                    (None, Some(text)) => Some(text.as_str()),
                    (None, None) => None,
                };
                let Some(content) = content else {
                    return Ok(None);
                };
                let visual = Visual::Text {
                    text: content.to_owned(),
                    style: self
                        .text_style(
                            font,
                            self.role_color(color, ctx, Role::of_style(font.base().unwrap_or(""))),
                        )
                        .stroked(self.stroke(stroke.as_ref(), ctx)),
                };
                self.scene.node(style, visual, &[])?
            }
            NodeKind::Icon { icon, size, color } => {
                let size = size * self.scale;
                let mut style = style;
                if layout.width.is_none() {
                    style.size.width = Dimension::length(size);
                }
                if layout.height.is_none() {
                    style.size.height = Dimension::length(size);
                }
                let visual = Visual::Icon {
                    icon: *icon,
                    size,
                    color: self.color(color, ctx),
                };
                self.scene.node(style, visual, &[])?
            }
            NodeKind::Preedit { .. } => {
                let Some(preedit) = &ctx.frame.preedit else {
                    return Ok(None);
                };
                self.preedit(spec, preedit, style, ctx)?
            }
            NodeKind::Annotation {
                bind,
                font,
                gloss,
                fresh,
                faint,
                pos,
                separator,
                tones,
                senses,
                stroke,
            } => {
                let Some(segments) = ctx.annotation(bind) else {
                    return Ok(None);
                };
                // 挑完是空的也留着节点（与没有译文时一样）：主题写了高度就照样占住这一行，几列对得齐
                let alt_from = ctx.row.and_then(|context| context.row.alt_gloss_from);
                let segments = pick_segments(segments, tones.as_deref(), *senses, alt_from);
                let stroke = self.stroke(stroke.as_ref(), ctx);
                // 点译文上屏的是哪个候选的译词：候选那一列的是它自己，横排底下那行是高亮的那个
                let candidate = match bind.as_str() {
                    "annotation" => ctx.row.map(|row| row.index),
                    "highlighted.annotation" => ctx.frame.highlighted,
                    _ => None,
                };
                let mut children = Vec::with_capacity(segments.len());
                let role = Role::of_style(font.base().unwrap_or("annotation"));
                for ((segment, tone), sense, alt) in segments {
                    let color = match tone {
                        // 码是词本身的属性，与译文同一个淡色
                        Tone::Gloss | Tone::Code => gloss,
                        // 设置里关掉「生词用醒目色标出」时，生词跟普通译词同一个色
                        Tone::Fresh if self.theme.fresh_words() => fresh,
                        Tone::Fresh => gloss,
                        Tone::Faint => faint,
                        Tone::Pos => pos.as_ref().unwrap_or(faint),
                        Tone::Separator => separator.as_ref().unwrap_or(faint),
                    };
                    let visual = Visual::Text {
                        text: segment.clone(),
                        style: self
                            .text_style(font, self.role_color(color, ctx, role))
                            .stroked(stroke),
                    };
                    let child = self.scene.node(Style::default(), visual, &[])?;
                    if let Some(candidate) = candidate
                        && !matches!(tone, Tone::Separator | Tone::Code)
                    {
                        self.sense_nodes.push((candidate, sense, alt, child));
                    }
                    children.push(child);
                }
                self.scene.node(
                    layout_style::flex(style, Direction::Row),
                    Visual::Group,
                    &children,
                )?
            }
            NodeKind::Frame { .. } | NodeKind::Use { .. } | NodeKind::Repeat { .. } => {
                return Ok(None);
            }
        };
        self.apply_layer(node, layout, ctx);
        Ok(Some(node))
    }

    /// 拼音各段 + 行尾留出光标宽度；光标绝对定位在光标前文字的宽度处，最后画、压在字形上。
    fn preedit(
        &mut self,
        spec: &NodeSpec,
        preedit: &Preedit,
        style: Style,
        ctx: Context,
    ) -> Result<NodeId, RenderError> {
        let NodeKind::Preedit {
            font,
            typed,
            rest,
            struck,
            caret,
            stroke,
        } = &spec.kind
        else {
            return self.scene.node(style, Visual::Group, &[]);
        };
        let stroke = self.stroke(stroke.as_ref(), ctx);
        let typed_style = self
            .preedit_style(font, self.role_color(typed, ctx, Some(Role::Preedit)))
            .stroked(stroke);
        let mut children = Vec::with_capacity(preedit.segments.len() + 2);
        for segment in &preedit.segments {
            let text_style = match segment.style {
                PreeditStyle::Typed => typed_style,
                PreeditStyle::Rest => self
                    .preedit_style(font, self.role_color(rest, ctx, Some(Role::Preedit)))
                    .stroked(stroke),
                // 辅码码段：与剩余拼音同色，再压一道下划线区分
                PreeditStyle::AuxCode => self
                    .preedit_style(font, self.role_color(rest, ctx, Some(Role::Preedit)))
                    .stroked(stroke)
                    .underlined(),
                PreeditStyle::Struck => self
                    .preedit_style(font, self.role_color(struck, ctx, Some(Role::Preedit)))
                    .stroked(stroke)
                    .struck(),
            };
            let visual = Visual::Text {
                text: segment.text.clone(),
                style: text_style,
            };
            children.push(self.scene.node(Style::default(), visual, &[])?);
        }
        let caret_width = caret.width * self.scale;
        let spacer = Style {
            size: taffy::Size {
                width: Dimension::length(caret_width),
                height: Dimension::length(0.0),
            },
            ..Style::default()
        };
        children.push(self.scene.node(spacer, Visual::Group, &[])?);
        let caret_x = self
            .text
            .measure(&preedit.before_cursor(), &typed_style)
            .width;
        let caret_style = Style {
            position: taffy::Position::Absolute,
            inset: taffy::Rect {
                left: taffy::LengthPercentageAuto::length(caret_x),
                right: taffy::LengthPercentageAuto::auto(),
                top: taffy::LengthPercentageAuto::length(0.0),
                bottom: taffy::LengthPercentageAuto::auto(),
            },
            size: taffy::Size {
                width: Dimension::length(caret_width),
                height: Dimension::length(typed_style.line_height),
            },
            ..Style::default()
        };
        let visual = Visual::solid(self.color(&caret.color, ctx), 0.0);
        children.push(self.scene.node(caret_style, visual, &[])?);
        self.scene.node(
            layout_style::flex(style, Direction::Row),
            Visual::Group,
            &children,
        )
    }
}

/// 按 `senses`（每种语言前几个义项，按分隔数，各语言各自计数）与 `tones`（只要哪几种片段）
/// 挑出要画的片段。每一项带：片段本身、它属于这门语言的第几个义项（前面有几个分隔符）、
/// 是不是第二种语言（`alt_from` 是第二种语言第一条译词在 annotation 里的下标）。
fn pick_segments<'a>(
    segments: &'a [(String, Tone)],
    tones: Option<&[ToneFilter]>,
    senses: Option<usize>,
    alt_from: Option<usize>,
) -> Vec<(&'a (String, Tone), usize, bool)> {
    let mut primary = 0usize;
    let mut alt = 0usize;
    let mut picked = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let is_alt = alt_from.is_some_and(|from| index >= from);
        let counter = if is_alt { &mut alt } else { &mut primary };
        if segment.1 == Tone::Separator {
            *counter += 1;
        }
        let sense = *counter;
        if senses.is_some_and(|limit| sense >= limit) {
            continue;
        }
        if tones.is_none_or(|tones| tones.iter().any(|tone| tone.matches(segment.1))) {
            picked.push((segment, sense, is_alt));
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::pick_segments;
    use crate::frame::Tone;
    use crate::theme::file::node::ToneFilter;

    fn texts(picked: Vec<(&(String, Tone), usize, bool)>) -> Vec<&str> {
        picked
            .iter()
            .map(|((text, _), _, _)| text.as_str())
            .collect()
    }

    fn segments() -> Vec<(String, Tone)> {
        [
            ("int. ", Tone::Pos),
            ("hello", Tone::Gloss),
            (" · ", Tone::Separator),
            ("int. ", Tone::Pos),
            ("hi", Tone::Fresh),
        ]
        .into_iter()
        .map(|(text, tone)| (text.to_owned(), tone))
        .collect()
    }

    #[test]
    fn picks_by_tone_and_sense() {
        let segments = segments();
        assert_eq!(texts(pick_segments(&segments, None, None, None)).len(), 5);
        assert_eq!(
            texts(pick_segments(
                &segments,
                Some(&[ToneFilter::Pos]),
                Some(1),
                None
            )),
            ["int. "]
        );
        assert_eq!(
            texts(pick_segments(
                &segments,
                Some(&[ToneFilter::Gloss, ToneFilter::Fresh]),
                None,
                None
            )),
            ["hello", "hi"]
        );
        assert_eq!(
            texts(pick_segments(&segments, None, Some(1), None)),
            ["int. ", "hello"]
        );
    }

    #[test]
    fn senses_count_per_language_and_flag_the_second() {
        // 第一种语言两条 + 分隔 + 标签 + 第二种语言两条
        let segments: Vec<(String, Tone)> = [
            ("hello", Tone::Gloss),
            (" · ", Tone::Separator),
            ("hi", Tone::Fresh),
            (" · ", Tone::Separator),
            ("英 ", Tone::Faint),
            ("你好", Tone::Gloss),
            (" · ", Tone::Separator),
            ("喂", Tone::Gloss),
        ]
        .into_iter()
        .map(|(text, tone)| (text.to_owned(), tone))
        .collect();
        let picked = pick_segments(&segments, None, None, Some(5));
        let shape: Vec<(&str, usize, bool)> = picked
            .iter()
            .map(|((text, _), sense, alt)| (text.as_str(), *sense, *alt))
            .collect();
        assert_eq!(
            shape,
            [
                ("hello", 0, false),
                (" · ", 1, false),
                ("hi", 1, false),
                (" · ", 2, false),
                ("英 ", 2, false),
                ("你好", 0, true),
                (" · ", 1, true),
                ("喂", 1, true),
            ]
        );
        // 每种语言各留第一条义项
        assert_eq!(
            texts(pick_segments(&segments, None, Some(1), Some(5))),
            ["hello", "你好"]
        );
    }
}
