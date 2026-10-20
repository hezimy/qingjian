//! 「外观」页：预览卡，外观（外观、主题），排布（排布、拼音显示位置），文字（渲染引擎、字体，候选 / 序号 / 译文 / 拼音行的字号、字重、颜色），动效。

mod themes;

use bamboo_slip::taffy::NodeId;
use bamboo_slip::{
    Button, Checkbox, Color, RenderError, SettingRow, Stepper, TextField, Ui, group, section_title,
};
use qingjian_platform::{Appearance, CandidateRenderer, LayoutMode, PreeditMode};

use crate::message::{FieldId, Message, Popup, SizeKind};
use crate::support::Feature;
use crate::view::Page;
use crate::view::parts::controls::{choice, select, toggle};

pub fn appearance(ui: &mut Ui<Message>, page: &Page) -> Result<Vec<NodeId>, RenderError> {
    let theme = &page.theme;
    let general = &page.settings.config.general;
    // 颜色框的占位提示：按当前外观取主题里那一档的值，候选字用 `text`，译文与拼音行用 `gloss`
    let render_theme = page.settings.theme(general.theme_id());
    let mut out = vec![themes::preview(ui, page)?];

    let appearance = choice(
        ui,
        page,
        "appearance",
        &Appearance::ALL,
        general.appearance(),
        Appearance::label,
        |value| Message::set("general", "appearance", value.key()),
    )?;
    let appearance = SettingRow::new("外观")
        .control(appearance)
        .build(ui, theme)?;
    let cards = themes::cards(ui, page)?;
    let themes = SettingRow::new("主题")
        .control(cards)
        .stacked()
        .build(ui, theme)?;
    // 窗口底色与高亮底色是写进 `[general]` 的对主题 `surface` / `accent` 两个变量的覆盖，跟主题卡片放一组
    let theme_surface = theme_color(render_theme.variable_color("surface"));
    let surface = color(
        ui,
        page,
        FieldId::SurfaceColor,
        "窗口背景色",
        &format!(
            "填 #RRGGBB，空用{theme_surface}。悬浮状态条的底色跟它一致，主题的圆角与投影不变。"
        ),
        &theme_surface,
        None,
    )?;
    let theme_highlight = theme_color(render_theme.variable_color("accent"));
    let highlight = color(
        ui,
        page,
        FieldId::HighlightColor,
        "高亮颜色",
        &format!(
            "候选列表里当前选中那一行的底色；填 #RRGGBB，空用{theme_highlight}。只改这一块底，行上文字（含译文）的颜色仍由主题定。"
        ),
        &theme_highlight,
        None,
    )?;
    out.push(section_title(ui, theme, "外观")?);
    out.push(group(ui, theme, &[appearance, themes, surface, highlight])?);

    let layout = choice(
        ui,
        page,
        "layout",
        &LayoutMode::ALL,
        general.layout,
        LayoutMode::label,
        |value| Message::set("general", "layout", value.key()),
    )?;
    let mut layout = SettingRow::new("排布").control(layout);
    if Feature::HorizontalGrid.here() {
        let grid = Checkbox::new(general.horizontal_grid, "横排时 ↑ / ↓ 展开成多行")
            .disabled(general.layout != LayoutMode::Horizontal)
            .build(
                ui,
                theme,
                "grid",
                Message::set("general", "horizontal_grid", !general.horizontal_grid),
            )?;
        layout = layout.below(grid);
    }
    let layout = layout.build(ui, theme)?;
    let preedit = choice(
        ui,
        page,
        "preedit",
        &PreeditMode::ALL,
        general.preedit,
        PreeditMode::label,
        |value| Message::set("general", "preedit", value.key()),
    )?;
    let preedit = SettingRow::new("拼音显示位置")
        .info("终端里行内拼音显示不正常时选「只在窗口」")
        .control(preedit)
        .build(ui, theme)?;
    out.push(section_title(ui, theme, "排布")?);
    out.push(group(ui, theme, &[layout, preedit])?);

    let renderer = choice(
        ui,
        page,
        "renderer",
        &CandidateRenderer::ALL,
        general.renderer,
        CandidateRenderer::label,
        |value| Message::set("general", "renderer", value.key()),
    )?;
    let renderer = SettingRow::new("渲染引擎")
        .control(renderer)
        .build(ui, theme)?;
    // 字体与字号只对青简渲染器生效
    let own = general.renderer == CandidateRenderer::Qingjian;
    let font_name = if general.font.is_empty() {
        "系统字体"
    } else {
        general.font.as_str()
    };
    let font = select(ui, page, Popup::Font, (font_name, &[]), 180.0, !own)?;
    let font = SettingRow::new("字体").control(font).build(ui, theme)?;
    let candidate = size(ui, page, SizeKind::Candidate, own)?;
    let candidate = SettingRow::new("候选字号")
        .control(candidate)
        .build(ui, theme)?;
    let candidate_weight = weight(ui, page, SizeKind::Candidate, own)?;
    let candidate_weight = SettingRow::new("候选字重")
        .control(candidate_weight)
        .build(ui, theme)?;
    let candidate_color = color(
        ui,
        page,
        FieldId::CandidateColor,
        "候选颜色",
        "填 #RRGGBB；空用主题的。只改常态，高亮候选的文字色仍由主题定。",
        &theme_color(render_theme.variable_color("text")),
        None,
    )?;
    let index = size(ui, page, SizeKind::Index, own)?;
    let index = SettingRow::new("序号字号")
        .control(index)
        .build(ui, theme)?;
    let annotation = size(ui, page, SizeKind::Annotation, own)?;
    let annotation = SettingRow::new("译文字号")
        .control(annotation)
        .build(ui, theme)?;
    let annotation_weight = weight(ui, page, SizeKind::Annotation, own)?;
    let annotation_weight = SettingRow::new("译文字重")
        .control(annotation_weight)
        .build(ui, theme)?;
    let annotation_color = color(
        ui,
        page,
        FieldId::AnnotationColor,
        "译文颜色",
        "填 #RRGGBB；空用主题的。",
        &theme_color(render_theme.variable_color("gloss")),
        None,
    )?;
    let input = size(ui, page, SizeKind::Input, own)?;
    let input = SettingRow::new("拼音行字号")
        .control(input)
        .build(ui, theme)?;
    let input_weight = weight(ui, page, SizeKind::Input, own)?;
    let input_weight = SettingRow::new("拼音行字重")
        .control(input_weight)
        .build(ui, theme)?;
    let customized = general.candidate_font_size.get().is_some()
        || general.annotation_font_size.get().is_some()
        || general.index_font_size.get().is_some()
        || general.input_font_size.get().is_some();
    let reset = Button::new("恢复主题字号")
        .disabled(!own || !customized)
        .build(ui, theme, "reset-sizes", Message::ResetSizes)?;
    let input_color = color(
        ui,
        page,
        FieldId::InputColor,
        "拼音行颜色",
        "填 #RRGGBB；空用主题的。只改正在敲的那一段。",
        &theme_color(render_theme.variable_color("gloss")),
        Some(reset),
    )?;
    out.push(section_title(ui, theme, "文字")?);
    out.push(group(
        ui,
        theme,
        &[
            renderer,
            font,
            candidate,
            candidate_weight,
            candidate_color,
            index,
            annotation,
            annotation_weight,
            annotation_color,
            input,
            input_weight,
            input_color,
        ],
    )?);

    let animations = toggle(
        ui,
        page,
        "过渡动画",
        ("general", "animations"),
        general.animations,
    )?;
    out.push(section_title(ui, theme, "动效")?);
    out.push(group(ui, theme, &[animations])?);
    Ok(out)
}

fn size(
    ui: &mut Ui<Message>,
    page: &Page,
    kind: SizeKind,
    enabled: bool,
) -> Result<NodeId, RenderError> {
    let value = page.settings.size(kind);
    let text = format!("{} pt", trim(value));
    let local = match kind {
        SizeKind::Candidate => "candidate-size",
        SizeKind::Annotation => "annotation-size",
        SizeKind::Index => "index-size",
        SizeKind::Input => "input-size",
    };
    Stepper::new(&text)
        .limits(value > 6.0, value < 72.0)
        .disabled(!enabled)
        .build(
            ui,
            &page.theme,
            local,
            Message::Size(kind, -1),
            Message::Size(kind, 1),
        )
}

/// 字重步进框：0 是「跟随主题」，其余按百位档位增删，范围 0–900。
fn weight(
    ui: &mut Ui<Message>,
    page: &Page,
    kind: SizeKind,
    enabled: bool,
) -> Result<NodeId, RenderError> {
    let value = page.settings.weight(kind);
    let text = if value == 0 {
        "跟随主题".to_owned()
    } else {
        format!("{} {}", weight_name(value), value)
    };
    let (key, local) = match kind {
        SizeKind::Candidate => ("candidate_weight", "candidate-weight"),
        SizeKind::Annotation => ("annotation_weight", "annotation-weight"),
        SizeKind::Input => ("input_weight", "input-weight"),
        // 序号没有独立字重，字重控件不会为它建出来
        SizeKind::Index => ("candidate_weight", "index-weight"),
    };
    Stepper::new(&text)
        .limits(value > 0, value < 900)
        .disabled(!enabled)
        .build(
            ui,
            &page.theme,
            local,
            Message::set("general", key, i64::from(value.saturating_sub(100))),
            Message::set(
                "general",
                key,
                i64::from(value.saturating_add(100).min(900)),
            ),
        )
}

/// 字重的常见叫法，取最接近的百位（与旧设置程序一致）。
fn weight_name(weight: u16) -> &'static str {
    match (f32::from(weight) / 100.0).round() as u16 {
        0 | 1 => "极细",
        2 => "特细",
        3 => "细",
        4 => "常规",
        5 => "中等",
        6 => "半粗",
        7 => "粗",
        8 => "特粗",
        _ => "极粗",
    }
}

/// 颜色框：填 `#RRGGBB`，空用主题的；取不到编辑状态时退化成一个只有标题的行（`below` 照挂）。
fn color(
    ui: &mut Ui<Message>,
    page: &Page,
    id: FieldId,
    title: &str,
    info: &str,
    placeholder: &str,
    below: Option<NodeId>,
) -> Result<NodeId, RenderError> {
    let own = page.settings.config.general.renderer == CandidateRenderer::Qingjian;
    let Some(edit) = page.settings.fields.get(&id) else {
        let mut row = SettingRow::new(title);
        if let Some(below) = below {
            row = row.below(below);
        }
        return row.build(ui, &page.theme);
    };
    let input = TextField::new(edit)
        .placeholder(placeholder)
        .width(120.0)
        .disabled(!own)
        .build(
            ui,
            &page.theme,
            ("field", id),
            move |edit| Message::Edit(id, edit),
            Some(Message::Commit(id)),
            Some(Message::Cancel(id)),
        )?;
    let mut row = SettingRow::new(title).info(info).control(input);
    if let Some(below) = below {
        row = row.below(below);
    }
    row.build(ui, &page.theme)
}

/// 颜色框的占位提示：主题当前用的色值。
fn theme_color(color: Color) -> String {
    format!("主题色 #{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

/// 整数不带小数点，其余留一位。
fn trim(value: f32) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}
