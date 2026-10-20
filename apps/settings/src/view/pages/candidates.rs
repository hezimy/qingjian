//! 「候选」页：每页几个、译文、输出字形，候选后面的 emoji / 颜文字 / 符号，排序（学习、本地整句模型），英文词。

use bamboo_slip::taffy::{FlexDirection, NodeId, Style};
use bamboo_slip::{Checkbox, RenderError, SettingRow, Stepper, Ui, Visual, group, section_title};
use qingjian_core::TraditionalVariant;
use qingjian_platform::{EmojiCount, MAX_PAGE_SIZE};

use crate::message::{Message, Popup};
use crate::view::Page;
use crate::view::parts::controls::{LANGUAGES, select, toggle_info, toggle_with};

pub fn candidates(ui: &mut Ui<Message>, page: &Page) -> Result<Vec<NodeId>, RenderError> {
    let theme = &page.theme;
    let config = &page.settings.config;
    let general = &config.general;
    let mut out = Vec::new();

    let size = general.page_size();
    let stepper = Stepper::new(&size.to_string())
        .limits(size > 1, size < MAX_PAGE_SIZE)
        .build(
            ui,
            theme,
            "page-size",
            Message::set("general", "page_size", (size - 1) as i64),
            Message::set("general", "page_size", (size + 1) as i64),
        )?;
    let language = learning_languages(ui, page)?;
    let traditional = select(
        ui,
        page,
        Popup::Traditional,
        (
            general.traditional().label(),
            &TraditionalVariant::ALL.map(TraditionalVariant::label),
        ),
        150.0,
        false,
    )?;
    let mut rows = vec![
        SettingRow::new("每页候选数")
            .control(stepper)
            .build(ui, theme)?,
        SettingRow::new("译文")
            .info("不显示时生词标记也关掉")
            .control(language)
            .build(ui, theme)?,
        SettingRow::new("输出字形")
            .info("只换字形、不换用语（软件 → 軟件）；台湾与香港的字形取舍不同（裡 / 裏）")
            .control(traditional)
            .build(ui, theme)?,
    ];
    // 词性标注的接线目前只有 Windows 候选窗（#430）；macOS / Linux 候选行接上后再放开
    if cfg!(target_os = "windows") {
        rows.push(
            toggle_info(
                ui,
                page,
                "译词显示词性",
                Some("关掉后候选旁只显示译词，不显示 v. / n. / adj. 等标注（如 develop 而非 v. develop）"),
                ("general", "translation_pos"),
                general.translation_pos,
                general.learning_language_off(),
            )?,
        );
    }
    // 熟词动态标灰也只有 Windows 候选窗读它（macOS / Linux 的候选窗还没接），别的平台上显示也没用
    if cfg!(target_os = "windows") {
        rows.push(
            toggle_info(
                ui,
                page,
                "熟词动态标灰",
                Some("看过的轮次还不到 3 轮的译词算生词，看过 3 轮以上算熟词。开着：熟词用译文颜色（灰），生词用主题的醒目色（青简绿、护眼绿是橙 #FF8D28）；关掉：熟词不标灰，所有译文都用这个醒目色，一屏全标出来。"),
                ("general", "fresh_words"),
                general.fresh_words,
                false,
            )?,
        );
    }
    out.push(section_title(ui, theme, "显示")?);
    out.push(group(ui, theme, &rows)?);

    let rows = vec![
        count(
            ui,
            page,
            Popup::Emoji,
            "emoji",
            "kaixin → 开心 😄",
            general.emoji,
        )?,
        count(
            ui,
            page,
            Popup::Kaomoji,
            "颜文字",
            "ku → 哭 (T_T)",
            general.kaomoji,
        )?,
        count(
            ui,
            page,
            Popup::Symbols,
            "符号",
            "yuedengyu → 约等于 ≈，alpha → α",
            general.symbols,
        )?,
    ];
    out.push(section_title(ui, theme, "候选后面")?);
    out.push(group(ui, theme, &rows)?);

    let rows = vec![
        toggle_info(
            ui,
            page,
            "学习输入习惯",
            Some("关掉后不再学，已学的照样用"),
            ("general", "learning"),
            general.learning,
            false,
        )?,
        toggle_info(
            ui,
            page,
            "本地整句模型",
            Some("在本机给整句重新排序，不联网"),
            ("model", "enabled"),
            config.model.enabled,
            false,
        )?,
    ];
    out.push(section_title(ui, theme, "排序")?);
    out.push(group(ui, theme, &rows)?);

    let rows = vec![
        toggle_info(
            ui,
            page,
            "打拼音时也给英文词",
            Some("hello、compa → company"),
            ("general", "english_in_chinese"),
            general.english_in_chinese,
            false,
        )?,
        toggle_with(
            ui,
            page,
            "中文排在英文词前面",
            ("general", "chinese_first"),
            general.chinese_first,
            !general.english_in_chinese,
        )?,
    ];
    out.push(section_title(ui, theme, "英文词")?);
    out.push(group(ui, theme, &rows)?);
    Ok(out)
}

/// 学习语言的两行勾选框：第一行「不显示译文」与头两门语言，第二行其余三门。
/// 最多勾两门，勾满后没勾的置灰；一门都不勾写回「不显示译文」。
fn learning_languages(ui: &mut Ui<Message>, page: &Page) -> Result<NodeId, RenderError> {
    let general = &page.settings.config.general;
    let off = general.learning_language_off();
    let mut selected: Vec<String> = Vec::new();
    if !off {
        selected.push(general.learning_language.trim().to_owned());
        if let Some(language) = general.secondary_learning_language() {
            selected.push(language.code().to_owned());
        }
    }
    let full = selected.len() >= 2;
    let mut boxes = vec![Checkbox::new(off, "不显示译文").build(
        ui,
        &page.theme,
        "learning-off",
        Message::LearnLanguage(qingjian_platform::LEARNING_LANGUAGE_OFF, !off),
    )?];
    for (code, label) in LANGUAGES {
        let on = selected.iter().any(|item| item == code);
        boxes.push(Checkbox::new(on, label).disabled(!on && full).build(
            ui,
            &page.theme,
            ("learning-language", code),
            Message::LearnLanguage(code, !on),
        )?);
    }
    let style = Style {
        flex_direction: FlexDirection::Row,
        gap: page.gap(16.0),
        ..Style::default()
    };
    let top = ui.scene().node(style.clone(), Visual::Group, &boxes[..3])?;
    let bottom = ui.scene().node(style, Visual::Group, &boxes[3..])?;
    ui.scene().node(
        Style {
            flex_direction: FlexDirection::Column,
            gap: page.gap(8.0),
            ..Style::default()
        },
        Visual::Group,
        &[top, bottom],
    )
}

/// 候选后面一次最多配几个：不显示、1–3 个。
fn count(
    ui: &mut Ui<Message>,
    page: &Page,
    popup: Popup,
    title: &str,
    example: &str,
    current: EmojiCount,
) -> Result<NodeId, RenderError> {
    let control = select(
        ui,
        page,
        popup,
        (
            EmojiCount::LABELS[usize::from(current.get())],
            &EmojiCount::LABELS,
        ),
        120.0,
        false,
    )?;
    SettingRow::new(title)
        .info(example)
        .control(control)
        .build(ui, &page.theme)
}
