//! 候选窗的「醒目色」（`@fresh`，青简绿 / 护眼绿是橙 #ff8d28）用在哪儿：按真画出来的位图断言。
//!
//! 两条都是用户看得见的行为，之前只靠肉眼在实机上验：开着「熟词动态标灰」时熟词回落常态译文色、
//! 只有生词用醒目色；关掉后熟词不再标灰、所有译文都用醒目色；敲错的拼音段（`PreeditStyle::Struck`）
//! 也用醒目色，不能是浅灰。这里各出一张位图，数一数橙色像素。

use qingjian_render::sample::annotated;
use qingjian_render::{
    FontLibrary, Frame, Layout, Mode, Pixmap, Preedit, PreeditSegment, PreeditStyle, Renderer,
    Theme, Tone,
};

/// 一行熟词、一行生词，不带拼音行（免得敲错段的橙色混进来）。
fn rows_only() -> Frame {
    Frame {
        preedit: None,
        rows: vec![
            annotated(
                0,
                "开发",
                &[("v. ", Tone::Pos), ("develop", Tone::Gloss)],
                false,
            ),
            annotated(1, "开", &[("v. ", Tone::Pos), ("open", Tone::Fresh)], false),
        ],
        highlighted: None,
        footer: None,
        sentence: None,
        status: None,
        mode: Mode::default(),
        columns: 0,
        column_ems: Vec::new(),
    }
}

/// 只有一行拼音，末尾那段是敲错的。
fn struck_preedit() -> Frame {
    Frame {
        preedit: Some(Preedit {
            segments: vec![
                PreeditSegment {
                    text: "kai".to_owned(),
                    style: PreeditStyle::Typed,
                },
                PreeditSegment {
                    text: "fs".to_owned(),
                    style: PreeditStyle::Struck,
                },
            ],
            cursor: 5,
        }),
        rows: vec![annotated(0, "开发", &[("develop", Tone::Gloss)], false)],
        highlighted: None,
        footer: None,
        sentence: None,
        status: None,
        mode: Mode::default(),
        columns: 0,
        column_ems: Vec::new(),
    }
}

/// 位图里「一眼是橙」的像素数。`@fresh` 是 #ff8d28；抗锯齿的字沿会往背景摊开，只认明显偏橙的那些，
/// 灰字（#000000a6）与白底（#ffffff / #F6F8F5）都落不进来。
fn orange_pixels(pixmap: &Pixmap) -> usize {
    pixmap
        .pixels()
        .iter()
        .filter(|pixel| {
            let (red, green, blue) = (
                i32::from(pixel.red()),
                i32::from(pixel.green()),
                i32::from(pixel.blue()),
            );
            pixel.alpha() == 255 && red > 200 && red - blue > 60 && green < 220
        })
        .count()
}

/// 画一帧；没有系统字体（CI 容器）时返回 `None`，调用方跳过。
fn draw(theme: &Theme, frame: &Frame) -> Option<Pixmap> {
    let library = FontLibrary::system("zh-CN").ok()?;
    let mut renderer = Renderer::new(library);
    renderer
        .render(frame, Layout::Vertical, theme, 2.0)
        .ok()
        .map(|rendered| rendered.pixmap)
}

#[test]
fn turning_fresh_words_off_makes_every_translation_stand_out() {
    let theme = Theme::builtin("qingjian", false).expect("内置主题");
    let Some(fresh_only) = draw(&theme.with_fresh_words(true), &rows_only()) else {
        eprintln!("没有系统字体，跳过");
        return;
    };
    let all = draw(&theme.with_fresh_words(false), &rows_only()).expect("画得出来");
    assert!(
        orange_pixels(&fresh_only) > 0,
        "开着「熟词动态标灰」时生词该用醒目色"
    );
    assert!(
        orange_pixels(&all) > orange_pixels(&fresh_only),
        "关掉后熟词不再标灰、也该用醒目色：橙色像素要多过只标生词那会儿"
    );
}

#[test]
fn a_mistyped_preedit_segment_is_not_faint() {
    let theme = Theme::builtin("qingjian", false).expect("内置主题");
    let Some(shot) = draw(&theme, &struck_preedit()) else {
        eprintln!("没有系统字体，跳过");
        return;
    };
    assert!(
        orange_pixels(&shot) > 0,
        "敲错的拼音段要用醒目色，不能是浅灰"
    );
}
