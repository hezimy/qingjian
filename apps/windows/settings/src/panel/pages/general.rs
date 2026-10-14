//! 「通用」页：学习语言、每页候选数、输入方案、英文模式候选。

use qingjian_platform::{MAX_PAGE_SIZE, Scheme, ShiftLetter, SwitchKey};
use windows_reactor::{
    Callback, CheckBox, ChildrenControl, ComboBox, ContentControl, NumberBox, Orientation,
    StackPanel, ToggleSwitch, View, ViewContext,
};

use crate::panel::controls::{feedback, field, index_of, page, section};
use crate::panel::{Message, Settings};

/// 学习语言：界面名 + 配置写法。最多同时勾两种；「不显示译文」由下面 `language_boxes` 单独给。
/// 加「不显示译文」共六项，控件列宽上限 360px，六项横排会被卡片裁掉后三个，故那里分两行、每行三个。
pub(crate) const LANGUAGES: [(&str, &str); 5] = [
    ("英语", "en"),
    ("日语", "ja"),
    ("西班牙语", "es"),
    ("越南语", "vi"),
    ("德语", "de"),
];

/// 输入方案：界面名 + 配置写法，直接照 [`Scheme::ALL`] 建，不另抄一份。
/// 数组长度取自 `ALL`，以后加方案时这里数组对不上就编不过。
pub(crate) const SCHEMES: [(&str, &str); Scheme::ALL.len()] = [
    (Scheme::ALL[0].label(), Scheme::ALL[0].key()),
    (Scheme::ALL[1].label(), Scheme::ALL[1].key()),
    (Scheme::ALL[2].label(), Scheme::ALL[2].key()),
    (Scheme::ALL[3].label(), Scheme::ALL[3].key()),
    (Scheme::ALL[4].label(), Scheme::ALL[4].key()),
    (Scheme::ALL[5].label(), Scheme::ALL[5].key()),
    (Scheme::ALL[6].label(), Scheme::ALL[6].key()),
    (Scheme::ALL[7].label(), Scheme::ALL[7].key()),
    (Scheme::ALL[8].label(), Scheme::ALL[8].key()),
    (Scheme::ALL[9].label(), Scheme::ALL[9].key()),
];

pub(crate) fn string_combo(
    options: &'static [(&str, &str)],
    current: &str,
    callback: Callback<Option<usize>>,
) -> ComboBox {
    ComboBox::new()
        .items_source(options.iter().map(|(label, _)| *label))
        .selected_index(index_of(options, current))
        .on_selection_changed(callback)
}

/// Shift+字母的下拉：选项直接由 [`ShiftLetter::ALL`] 生成，免得再抄一份表（顺序要和它一致）。
fn shift_letter_combo(current: ShiftLetter, callback: Callback<Option<usize>>) -> ComboBox {
    ComboBox::new()
        .items_source(ShiftLetter::ALL.iter().map(|mode| mode.label()))
        .selected_index(ShiftLetter::ALL.iter().position(|mode| *mode == current))
        .on_selection_changed(callback)
}

pub(crate) fn view(settings: &Settings, context: &mut ViewContext<Settings>) -> View {
    let g = &settings.config.general;
    let english_only_in_apps = !settings.config.apps.english_candidates_everywhere();
    let rows = [
        field(
            "学习语言",
            "候选旁显示译词，可同时勾选两种（最多两种）。第一种用「快捷键」页的「第一个译词」修饰键加数字上屏（缺省 Ctrl+数字），\
             第二种用 Ctrl+Alt+数字，也可以直接点候选旁那条译文。勾「不显示译文」则关掉译文、生词标记与释义兜底。",
            language_boxes(settings, context),
        ),
        field(
            "每页候选数",
            "",
            NumberBox::new()
                .minimum(1.0)
                .maximum(MAX_PAGE_SIZE as f64)
                .value(g.page_size as f64)
                .on_value_changed(context.callback(Message::PageSize)),
        ),
        field(
            "拼音方案",
            "全拼、五套双拼、大千注音，或关（只用下面的五笔）。\
             双拼下 v、u、i 是音节键，表达式与问字模式改用 Shift+V、Shift+U 进（微软、搜狗方案的 ; 键是 ing）；\
             注音下 v、u、i 也是按键，只能用 ? 开头进。",
            string_combo(
                &SCHEMES,
                g.scheme().key(),
                context.callback(Message::Scheme),
            ),
        ),
        field(
            "双拼在输入框显示原始按键",
            "勾上后双拼模式下输入框（光标处）显示敲击的英文字母，回车可直接上屏；候选窗口顶部的拼音行照旧显示解码全拼。",
            ToggleSwitch::new()
                .is_on(g.shuangpin_raw_preedit)
                .is_enabled(g.scheme().is_shuangpin())
                .on_toggled(context.callback(Message::ShuangpinRawPreedit)),
        ),
        field(
            "五笔（86 版）",
            "与拼音方案同时开着就是混输：编码打全的五笔词在前，打不出的字直接打拼音。\
             单用五笔请把拼音方案关掉；第 5 个字母起五笔查不到东西，自动只剩拼音。\
             译词、生词记录与学习照常。",
            ToggleSwitch::new()
                .is_on(g.wubi())
                .on_toggled(context.callback(Message::Wubi)),
        ),
        field(
            "繁体输出",
            "打字时将候选词转换为繁体中文。",
            ToggleSwitch::new()
                .is_on(g.traditional)
                .on_toggled(context.callback(Message::Traditional)),
        ),
        field(
            "候选后面显示 emoji",
            "开着时有对应 emoji 的词后面跟一两个（kaixin → 开心 😄）；关掉候选里只有字词。",
            ToggleSwitch::new()
                .is_on(g.emoji)
                .on_toggled(context.callback(Message::Emoji)),
        ),
        field(
            "中文模式标点转全角",
            "没在打拼音时敲 , . ? ! 等出「，。？！」，数字后面的点保持半角；悬浮状态条的「，。」格也能切，切的是当前模式那份。",
            ToggleSwitch::new()
                .is_on(g.full_width_punctuation)
                .on_toggled(context.callback(Message::FullWidthPunctuation)),
        ),
        field(
            "英文模式标点转全角",
            "中英各记一份，缺省英文半角。",
            ToggleSwitch::new()
                .is_on(g.english_full_width_punctuation)
                .on_toggled(context.callback(Message::EnglishFullWidthPunctuation)),
        ),
        field(
            "组句中标点先上屏候选",
            "打拼音时敲 , . ? ! 等先把高亮候选上屏、再补该标点（nihao, 一气打完「你好，」）；关掉后标点进英文直输段（老行为）。",
            ToggleSwitch::new()
                .is_on(g.punct_commits)
                .on_toggled(context.callback(Message::PunctCommits)),
        ),
        field(
            "英文候选与补全",
            "Tab 或方向键选词；空格、回车、标点仍原样上屏敲的字母，不选词时与直接打字一样。",
            ToggleSwitch::new()
                .is_on(g.english_candidates)
                .on_toggled(context.callback(Message::EnglishCandidates)),
        ),
        field(
            "只在浏览器、聊天和文档类应用里给",
            "Chrome、Edge、微信、QQ、Word、记事本等；别的应用（终端、代码编辑器、Photoshop 这类靠单键快捷键的软件）里字母原样交给应用。名单可在配置文件里改。",
            ToggleSwitch::new()
                .is_on(english_only_in_apps)
                .is_enabled(g.english_candidates)
                .on_toggled(context.callback(Message::EnglishOnlyInApps)),
        ),
        field(
            "输入拼音时也给英文词和补全",
            "整段是英文词（hello）或像英文词的开头（compa → company）时给英文候选；关掉就只出中文，英文模式不受影响。",
            ToggleSwitch::new()
                .is_on(g.english_in_chinese)
                .on_toggled(context.callback(Message::EnglishInChinese)),
        ),
        field(
            "输入拼音时中文候选排在英文词前面",
            "开着时整段输入是英文词时（hello、key）英文词排第二，空格上屏的仍是中文；关着（缺省）拼音不成立的输入英文词排第一。",
            ToggleSwitch::new()
                .is_on(g.chinese_first)
                .is_enabled(g.english_in_chinese)
                .on_toggled(context.callback(Message::ChineseFirst)),
        ),
        field(
            "中文模式下的 Shift + 字母",
            "「交给应用」是临时打英文（与以前一致）：拼音先上屏，这个键归应用；\
             「进组句」把它收进拼音缓冲区，匹配时按小写算，所以 Cpan 与 cpan 一样能出「C盘」。",
            shift_letter_combo(g.shift_letter, context.callback(Message::ShiftLetter)),
        ),
        field(
            "中英切换键",
            "勾上的键都能在中英之间切换，可以多选，改完立刻生效。打字时容易误触 Shift 的话改勾「单击 Ctrl」；一个都不勾时只剩任务栏 / 悬浮状态条上的「中」「英」按钮。\
             「Ctrl + Space（系统）」是 Windows 的设置，由 Windows 管理，也会影响微软拼音等简体中文输入法。",
            switch_key_boxes(settings, context),
        ),
        field(
            "启用内置英文模式",
            "关掉后青简固定中文模式：切换键与任务栏、悬浮状态条上的「中」「英」按钮都不再切到英文，需要英文时用系统快捷键（Win + Space）切到别的输入法。",
            ToggleSwitch::new()
                .is_on(g.english_mode)
                .on_toggled(context.callback(Message::EnglishMode)),
        ),
        field(
            "默认英文模式",
            "青简启动时使用英文；按应用记忆时，没有记录的应用也从英文开始。更改这两项会重置已记住的状态。",
            ToggleSwitch::new()
                .is_on(g.default_english)
                .is_enabled(g.english_mode)
                .on_toggled(context.callback(Message::DefaultEnglish)),
        ),
        field(
            "按应用记住中英模式",
            "切回应用时恢复它上次的中英状态，同一应用的窗口共用。重启青简后重新使用默认模式；关掉则所有应用共用一份。",
            ToggleSwitch::new()
                .is_on(g.remember_mode_per_app)
                .is_enabled(g.english_mode)
                .on_toggled(context.callback(Message::RememberModePerApp)),
        ),
    ];
    let mut rows = rows.into_iter();
    page(
        "通用",
        StackPanel::new().spacing(24.0).children([
            section("输入与候选", rows.by_ref().take(7)),
            section("标点", rows.by_ref().take(3)),
            section("中英文输入", rows.by_ref().take(9)),
            feedback(&settings.notice),
        ]),
    )
}

/// 学习语言：每门一个勾选框 + 「不显示译文」。控件列宽上限 360px，六项横排会被卡片裁掉后三个，
/// 所以分两行、每行三个（行序即 [`LANGUAGES`] 顺序）。最多同时勾两种（勾满后其余置灰）。
fn language_boxes(settings: &Settings, context: &mut ViewContext<Settings>) -> View {
    let general = &settings.config.general;
    let off = general.learning_language_off();
    let mut selected = Vec::new();
    if !off {
        selected.push(general.learning_language.trim().to_owned());
        if let Some(language) = general.secondary_learning_language() {
            selected.push(language.code().to_owned());
        }
    }
    let full = selected.len() >= 2;
    let off_box: View = CheckBox::new()
        .is_checked(off)
        .on_is_checked_changed(context.callback(|on| Message::LearningPick("off", on)))
        .content("不显示译文")
        .into();
    let row = |boxes: [View; 3]| -> View {
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children(boxes)
    };
    StackPanel::new().spacing(6.0).children([
        row([
            off_box,
            language_box(LANGUAGES[0], &selected, full, context),
            language_box(LANGUAGES[1], &selected, full, context),
        ]),
        row([
            language_box(LANGUAGES[2], &selected, full, context),
            language_box(LANGUAGES[3], &selected, full, context),
            language_box(LANGUAGES[4], &selected, full, context),
        ]),
    ])
}

/// 一门学习语言的勾选框：已在选中列表里就勾上；已经勾满两种且自己不在此列时置灰。
fn language_box(
    (label, code): (&'static str, &'static str),
    selected: &[String],
    full: bool,
    context: &mut ViewContext<Settings>,
) -> View {
    let on = selected.iter().any(|item| item == code);
    CheckBox::new()
        .is_checked(on)
        .is_enabled(on || !full)
        .on_is_checked_changed(context.callback(move |on| Message::LearningPick(code, on)))
        .content(label)
}

/// 中英切换键：系统 Ctrl + Space 在最上面，其余青简切换键纵排，避免在窄窗口与高缩放下挤出卡片。
fn switch_key_boxes(settings: &Settings, context: &mut ViewContext<Settings>) -> View {
    let keys = settings.config.shortcut.switch_mode;
    let boxes = SwitchKey::ALL.map(|key| {
        CheckBox::new()
            .is_checked(keys.contains(key))
            .on_is_checked_changed(context.callback(move |on| Message::SwitchKey(key, on)))
            .content(key.label())
    });
    StackPanel::new().spacing(4.0).children([
        CheckBox::new()
            .is_checked(crate::panel::system_hotkey::ctrl_space_enabled())
            .on_is_checked_changed(context.callback(Message::SystemCtrlSpace))
            .content("Ctrl + Space（系统）"),
        StackPanel::new()
            .orientation(Orientation::Vertical)
            .spacing(4.0)
            .children(boxes),
    ])
}
