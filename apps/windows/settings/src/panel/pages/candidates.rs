//! 「候选窗口」页：外观、主题、排布、渲染引擎、字体与字号、颜色、过渡动画、拼音显示位置、悬浮状态条。

use qingjian_platform::{Appearance, CandidateRenderer, FontSize, LayoutMode, PreeditMode};
use qingjian_render::{Color, Theme, ThemeLibrary, candidate_weights};
use windows_reactor::*;

use crate::panel::controls::{field, mode_combo, page};
use crate::panel::{Message, Settings};

/// 字号框：不限范围；没设过显示主题的字号（用户知道从哪个数开始调）。
fn size_box(size: FontSize, theme: Option<f32>, callback: Callback<Option<f64>>) -> NumberBox {
    NumberBox::new()
        .value(size.get().or(theme).map_or(f64::NAN, f64::from))
        .on_value_changed(callback)
}

/// 字重下拉的条目：只列这台机器上真有的档位，0 是「跟随主题」。
/// 填过的值不在列表里（旧配置里的中间值、换过字体）也列出来并标出来，用户才知道它其实落不到实处。
fn weight_items(weight: u16, available: &[u16]) -> (Vec<String>, Vec<u16>) {
    let mut values = vec![0u16];
    values.extend_from_slice(available);
    if weight != 0 && !values.contains(&weight) {
        values.push(weight);
    }
    let items = values
        .iter()
        .map(|value| weight_label(*value, available))
        .collect();
    (items, values)
}

/// 字重条目的写法。
fn weight_label(weight: u16, available: &[u16]) -> String {
    if weight == 0 {
        return "跟随主题".to_owned();
    }
    let name = weight_name(weight);
    if available.contains(&weight) {
        format!("{name}（{weight}）")
    } else {
        format!("{name}（{weight}，本机没有这一档）")
    }
}

/// 字重的常见叫法，取最接近的百位。
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

/// 字重下拉：列这台机器上能真正画出来的档位（见 [`candidate_weights`]）。
fn weight_combo(
    weight: u16,
    style: &'static str,
    available: &[u16],
    context: &mut ViewContext<Settings>,
) -> ComboBox {
    let (items, values) = weight_items(weight, available);
    let selected = values
        .iter()
        .position(|value| *value == weight)
        .unwrap_or(0);
    ComboBox::new()
        .items_source(items)
        .selected_index(selected)
        .on_selection_changed(context.callback(move |index: Option<usize>| {
            Message::FontWeight(
                style.to_owned(),
                index.and_then(|i| values.get(i)).copied().unwrap_or(0),
            )
        }))
}

/// 字体下拉：第 0 项是「系统字体」（写空串），其后是本机字族。
/// 配置里写的字体本机没装也列出来并标一下——那种情况下渲染器其实用的是系统字体。
///
/// 用下拉而不是可输入的框：这台机器上有几百个字族，可输入的框只在打字时才弹候选，
/// 点开是空的（WinUI 的 AutoSuggestBox 就这样），反而挑不到。
fn font_combo(font: &str, families: &[String], context: &mut ViewContext<Settings>) -> ComboBox {
    let current = font.trim();
    let mut values: Vec<String> = Vec::with_capacity(families.len() + 2);
    values.push(String::new());
    values.extend(families.iter().cloned());
    if !current.is_empty()
        && !families
            .iter()
            .any(|family| family.eq_ignore_ascii_case(current))
    {
        values.push(current.to_owned());
    }
    let items: Vec<String> = values
        .iter()
        .map(|value| {
            if value.is_empty() {
                "系统字体".to_owned()
            } else if value.eq_ignore_ascii_case(current) && !families.iter().any(|f| f == value) {
                format!("{value}（本机没装，回落到系统字体）")
            } else {
                value.clone()
            }
        })
        .collect();
    let selected = values
        .iter()
        .position(|value| value.eq_ignore_ascii_case(current))
        .unwrap_or(0);
    ComboBox::new()
        .width(280.0)
        .items_source(items)
        .selected_index(selected)
        .on_selection_changed(context.callback(move |index: Option<usize>| {
            Message::Font(
                index
                    .and_then(|i| values.get(i))
                    .cloned()
                    .unwrap_or_default(),
            )
        }))
}

/// 颜色的 `#RRGGBB` 写法（丢掉 alpha，与色值的填法一致）。
fn hex(color: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

/// 主题当前用的色值，做颜色框的占位提示。
fn theme_color(value: Color) -> String {
    format!("主题色 {}", hex(value))
}

/// 颜色框：`#RRGGBB`；空为用主题色，占位提示里写明主题当前用的色值。
fn color_box(
    color: &str,
    style: &'static str,
    placeholder: &str,
    context: &mut ViewContext<Settings>,
) -> TextBox {
    TextBox::new()
        .text(color.to_owned())
        .placeholder_text(placeholder.to_owned())
        .on_text_changed(context.callback(move |text| Message::FontColor(style.to_owned(), text)))
}

/// 「字体」留空时到底用的哪个字体：主题给候选字单独指定了字族就写出来（如樱花的宋体），
/// 没指定就是界面字体。不写这一句，用着樱花的人会以为「系统字体」就是微软雅黑。
fn theme_font(theme: &Theme) -> String {
    let families = theme.style_families("candidate");
    if families.is_empty() {
        return "系统字体".to_owned();
    }
    families
        .iter()
        .map(|family| {
            if family == "system" {
                "系统字体".to_owned()
            } else {
                family.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

/// 三个字重下拉共用的说明。
const WEIGHT_HINT: &str = "下拉里只列这台机器上真有的档位（候选字与译文主要是汉字，档位由中文字体定）；「跟随主题」用主题写的字重。";

/// 「熟词动态标灰」的说明。名字就是这个开关做的事：**开着才标灰**，关掉就不标灰、一屏全用醒目色。
/// 名字与行为方向必须一致——叫「生词标色」时老是让人以为开着才上色（见 issue）。
const FRESH_HINT: &str = "看过的轮次还不到 3 轮的译词算生词，看过 3 轮以上算熟词。开着：熟词用译文颜色（灰），生词用主题的醒目色（青简绿、护眼绿是橙 #FF8D28）；关掉：熟词不标灰，所有译文都用这个醒目色，一屏全标出来。";

/// 「渲染引擎」的说明：切到系统绘制时，把哪些设置从此不生效一次说清。
const RENDERER_HINT: [&str; 2] = [
    "青简渲染器让候选窗口在各平台一致。",
    "青简渲染器让候选窗口在各平台一致。\n\n当前是「系统绘制」：候选窗口由系统的 GDI 画法出，下面只有「主题」与「过渡动画」不生效（主题的配色与装饰、高亮滑入与循环动画都是渲染器画的）；字体、字号、字重、颜色、窗口底色照上面画。",
];

pub(crate) fn view(settings: &Settings, context: &mut ViewContext<Settings>) -> View {
    let g = &settings.config.general;
    // 内置主题加用户主题目录里的，每次画这一页都重列（新放进去的主题回到这页就能看到）
    let themes = ThemeLibrary::load(qingjian_platform::dirs::themes_dir().as_deref());
    // 颜色框的占位提示按当前外观取主题里那一档的值（「跟随系统」按浅色算）；
    // 外观被主题锁住时 `with_dark` 会听主题的，与真正画出来的一致
    let theme = themes
        .resolve(g.theme_id(), false)
        .with_dark(matches!(g.appearance(), Appearance::Dark));
    // 主题与过渡动画只对青简渲染器生效；字体、字号、字重、颜色两套画法都认
    let live = g.renderer == CandidateRenderer::Qingjian;
    // 字重下拉只列本机真有的档位：候选字与译文主要是汉字，档位由中文字体定
    let font_files = if g.font.trim().is_empty() {
        Vec::new()
    } else {
        qingjian_render::system_fonts::family_files(g.font.trim())
    };
    let weights = candidate_weights("zh-CN", &font_files);
    let font_hint = format!(
        "留空时的「系统字体」到底是谁：主题给候选字单独指定了字族就用它（当前主题：{}），没指定就用界面字体（微软雅黑 UI）。本机没装的字体自动回到系统字体。",
        theme_font(&theme)
    );

    let rows = [
        field(
            "外观",
            "跟随系统时按 Windows 的浅色 / 深色设置切换。",
            mode_combo(
                &Appearance::ALL,
                g.appearance(),
                Appearance::label,
                context.callback(Message::Appearance),
            ),
        ),
        field(
            "主题",
            "只对青简渲染器生效；樱花只有浅色，其余主题按上面的外观切换浅色与深色。",
            ComboBox::new()
                .items_source(themes.themes().iter().map(|theme| theme.name().to_owned()))
                .selected_index(
                    themes
                        .themes()
                        .iter()
                        .position(|theme| theme.id() == g.theme_id())
                        .unwrap_or(0),
                )
                .on_selection_changed(context.callback(Message::Theme))
                .is_enabled(live),
        ),
        field(
            "排布",
            "横排时只给高亮的候选显示译词。",
            mode_combo(
                &LayoutMode::ALL,
                g.layout,
                LayoutMode::label,
                context.callback(Message::Layout),
            ),
        ),
        field(
            "渲染引擎",
            RENDERER_HINT[usize::from(!live)],
            mode_combo(
                &CandidateRenderer::ALL,
                g.renderer,
                CandidateRenderer::label,
                context.callback(Message::Renderer),
            ),
        ),
        field(
            "字体",
            &font_hint,
            font_combo(&g.font, &settings.families, context),
        ),
        field(
            "候选字号",
            "单位是点，清空或填 0 回到主题的字号；行高跟着缩放。",
            size_box(
                g.candidate_font_size,
                theme.theme_size("candidate"),
                context.callback(Message::CandidateFontSize),
            ),
        ),
        field(
            "候选字重",
            WEIGHT_HINT,
            weight_combo(g.candidate_weight, "candidate", &weights, context),
        ),
        field(
            "候选颜色",
            &format!(
                "填 #RRGGBB（如 #1F6FEB）；空用{}。只改常态，高亮候选的文字色仍由主题定。",
                theme_color(theme.variable_color("text"))
            ),
            color_box(
                &g.candidate_color,
                "candidate",
                &theme_color(theme.variable_color("text")),
                context,
            ),
        ),
        field(
            "译文字号",
            "单位是点，清空或填 0 回到主题的字号。",
            size_box(
                g.annotation_font_size,
                theme.theme_size("annotation"),
                context.callback(Message::AnnotationFontSize),
            ),
        ),
        field(
            "译文字重",
            WEIGHT_HINT,
            weight_combo(g.annotation_weight, "annotation", &weights, context),
        ),
        field(
            "译文颜色",
            &format!(
                "填 #RRGGBB；空用{}。填了以后上面的「熟词动态标灰」也一并被盖掉。",
                theme_color(theme.variable_color("gloss"))
            ),
            color_box(
                &g.annotation_color,
                "annotation",
                &theme_color(theme.variable_color("gloss")),
                context,
            ),
        ),
        field(
            "熟词动态标灰",
            FRESH_HINT,
            ToggleSwitch::new()
                .is_on(g.fresh_words)
                .on_toggled(context.callback(Message::FreshWords)),
        ),
        field(
            "拼音行字号",
            "录入框里正在敲的拼音的字号，单位是点；清空或填 0 回到主题的字号。不跟候选字缩放。",
            size_box(
                g.input_font_size,
                theme
                    .theme_size("input")
                    .or_else(|| theme.theme_size("preedit"))
                    .or_else(|| theme.theme_size("annotation")),
                context.callback(Message::InputFontSize),
            ),
        ),
        field(
            "拼音行字重",
            WEIGHT_HINT,
            weight_combo(g.input_weight, "input", &weights, context),
        ),
        field(
            "拼音行颜色",
            &format!(
                "填 #RRGGBB；空用{}。只改正在敲的那一段，后面没敲的与辅码段仍用主题的淡色。",
                theme_color(theme.variable_color("gloss"))
            ),
            color_box(
                &g.input_color,
                "input",
                &theme_color(theme.variable_color("gloss")),
                context,
            ),
        ),
        field(
            "窗口背景色",
            &format!(
                "填 #RRGGBB，空用{}。悬浮状态条的底色跟它一致，主题的圆角与投影不变。",
                theme_color(theme.surface_color())
            ),
            color_box(
                &g.surface_color,
                "surface",
                &theme_color(theme.surface_color()),
                context,
            ),
        ),
        field(
            "高亮颜色",
            &format!(
                "候选列表里当前选中那一行的底色；填 #RRGGBB，空用{}。只改这一块底，行上文字（含译文）的颜色仍由主题定。",
                theme_color(theme.highlight_color())
            ),
            color_box(
                &g.highlight_color,
                "highlight",
                &theme_color(theme.highlight_color()),
                context,
            ),
        ),
        field(
            "",
            "把候选字、译文、拼音行的字号、字重、颜色与候选高亮、窗口底色都清回主题的。",
            Button::new()
                .on_click(context.message(Message::ResetFontSizes))
                .content("恢复主题字号与颜色"),
        ),
        field(
            "过渡动画",
            "高亮换候选时滑过去、主题里的循环动画。关掉后直接跳到位；系统关了「动画效果」时也不播。只对青简渲染器生效。",
            ToggleSwitch::new()
                .is_on(g.animations)
                .on_toggled(context.callback(Message::Animations))
                .is_enabled(live),
        ),
        field(
            "拼音显示",
            "「只在候选窗口」时正在敲的拼音不显示在应用里，终端或行内拼音不正常的应用可以选它。",
            mode_combo(
                &PreeditMode::ALL,
                g.preedit,
                PreeditMode::label,
                context.callback(Message::Preedit),
            ),
        ),
        field(
            "悬浮状态条",
            "桌面上常驻、可拖动的小条：点「中 / 英」切换模式（开着双拼时还显示方案名），点「，。」切全角 / 半角标点，点齿轮打开设置。只在当前输入法是青简时显示，拖到哪下次还在哪。",
            ToggleSwitch::new()
                .is_on(settings.config.status_bar.enabled)
                .on_toggled(context.callback(Message::StatusBar)),
        ),
    ];
    page("候选窗口", StackPanel::new().spacing(16.0).children(rows))
}
