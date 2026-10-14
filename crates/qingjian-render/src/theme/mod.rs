//! 主题：一份 `theme.json`（图层树 + 组件 + 颜色变量 + 文字样式）加当前外观（浅色 / 深色）。格式见 `docs/design/theme.md`；
//! 文件可以写注释与尾逗号（见 `jsonc.rs`）。
//!
//! 内置主题随 crate 编进来（`themes/<id>/theme.json`），第一次用到时解析一次；用户主题用 [`Theme::from_json`]。
//! 主题可以 `"extends": "<内置主题 id>"`，只写要改的部分（见 `extends.rs`）。
//! 渲染时引用不到的颜色、样式退回缺省值，加载时 [`validate`] 先把这类问题记成警告。

mod assets;
mod error;
mod extends;
pub(crate) mod file;
mod font;
mod jsonc;
mod library;
mod svg_image;
mod text_sizes;
mod validate;

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use tiny_skia::Pixmap;

use crate::color::Color;

pub use error::ThemeError;
pub(crate) use font::{FamilyId, FontFamilies};
pub use font::{FontSpec, FontWeight};
pub use library::ThemeLibrary;
pub(crate) use text_sizes::Role;
pub use text_sizes::TextSizes;

use assets::Assets;
use file::{ColorRef, FontRef, LockedAppearance, SCHEMA, ThemeFile};
pub(crate) use svg_image::SvgImage;

/// 内置主题编进程序的图片：主题里写的路径与文件内容。
type EmbeddedFiles = &'static [(&'static str, &'static [u8])];

/// 内置主题：id、源文件与图片，按设置界面列出的顺序；第一个是缺省主题。
const BUILTINS: [(&str, &str, EmbeddedFiles); 5] = [
    (
        "qingjian",
        include_str!("../../themes/qingjian/theme.json"),
        &[],
    ),
    (
        "system-blue",
        include_str!("../../themes/system-blue/theme.json"),
        &[],
    ),
    (
        "wechat",
        include_str!("../../themes/wechat/theme.json"),
        &[],
    ),
    (
        "sakura",
        include_str!("../../themes/sakura/theme.json"),
        &[
            (
                "images/character.png",
                include_bytes!("../../themes/sakura/images/character.png"),
            ),
            (
                "images/charm.png",
                include_bytes!("../../themes/sakura/images/charm.png"),
            ),
            (
                "images/cat.png",
                include_bytes!("../../themes/sakura/images/cat.png"),
            ),
            (
                "images/blossom.png",
                include_bytes!("../../themes/sakura/images/blossom.png"),
            ),
            (
                "images/bow.svg",
                include_bytes!("../../themes/sakura/images/bow.svg"),
            ),
            (
                "images/crown.svg",
                include_bytes!("../../themes/sakura/images/crown.svg"),
            ),
            (
                "images/heart.svg",
                include_bytes!("../../themes/sakura/images/heart.svg"),
            ),
            (
                "images/sparkle.svg",
                include_bytes!("../../themes/sakura/images/sparkle.svg"),
            ),
            (
                "images/petal.svg",
                include_bytes!("../../themes/sakura/images/petal.svg"),
            ),
            (
                "images/moon.svg",
                include_bytes!("../../themes/sakura/images/moon.svg"),
            ),
            (
                "images/chevron-left.svg",
                include_bytes!("../../themes/sakura/images/chevron-left.svg"),
            ),
            (
                "images/chevron-right.svg",
                include_bytes!("../../themes/sakura/images/chevron-right.svg"),
            ),
        ],
    ),
    (
        "eyecare",
        include_str!("../../themes/eyecare/theme.json"),
        &[
            (
                "images/branch.svg",
                include_bytes!("../../themes/eyecare/images/branch.svg"),
            ),
            (
                "images/meadow.svg",
                include_bytes!("../../themes/eyecare/images/meadow.svg"),
            ),
        ],
    ),
];

/// 最底层的主题：没写 `extends` 的主题都以它为底，它自己不再往下找。
const ROOT_THEME: &str = "qingjian";

/// 引用不到的文字样式退回这个（点）。
const FALLBACK_FONT: FontSpec = FontSpec::new(16.0, 19.0);

#[derive(Debug, Clone)]
pub struct Theme {
    /// 解析后的主题文件，同一主题的浅色 / 深色共用。
    file: Arc<ThemeFile>,

    /// 用深色那一套值。
    dark: bool,

    /// 图片与字体素材（主题目录里的），同一主题的浅色 / 深色共用。
    assets: Arc<Assets>,

    /// 字族表与命名样式，同一主题的浅色 / 深色共用。
    families: Arc<FontFamilies>,

    /// 设置里的字号，盖过主题的。
    sizes: TextSizes,

    /// 设置里关了过渡动画时为 `false`：过渡直接跳到终点，循环动画停在第一帧。
    animations: bool,

    /// 设置里的候选高亮底色，盖过主题变量 `accent`（高亮行那块底）。`None` 用主题的。
    highlight: Option<Color>,

    /// 设置里的候选窗口底色，盖过主题变量 `surface`。`None` 用主题的。
    surface: Option<Color>,

    /// 是否只给生词上主题的醒目色（`@fresh`）。关掉后不分生词熟词，所有译文都用它。
    fresh_words: bool,
}

impl Theme {
    /// 缺省内置主题，浅色。
    pub fn light() -> Self {
        Self::default_builtin().clone()
    }

    /// 缺省内置主题，深色。
    pub fn dark() -> Self {
        Self::default_builtin().with_dark(true)
    }

    /// 按 id 找内置主题；没有这个 id 时为 `None`。
    pub fn builtin(id: &str, dark: bool) -> Option<Self> {
        builtins()
            .iter()
            .find(|theme| theme.id() == id)
            .map(|theme| theme.with_dark(dark))
    }

    /// 全部内置主题（浅色），按设置界面列出的顺序。
    pub fn builtins() -> &'static [Theme] {
        builtins()
    }

    /// 从 `theme.json` 的内容读主题。可以 `extends` 内置主题；引用不到的名字只记警告。
    /// 没写 `extends` 时以青简绿为底（青简绿自己除外）。
    pub fn from_json(json: &str, dark: bool) -> Result<Self, ThemeError> {
        let value = extends::resolve(json, &builtin_source, ROOT_THEME)?;
        let file: ThemeFile = serde_json::from_value(value)?;
        if file.schema > SCHEMA {
            tracing::warn!(
                id = file.meta.id,
                schema = file.schema,
                supported = SCHEMA,
                "主题格式比当前版本新，只按认得的部分画"
            );
        }
        for problem in validate::problems(&file) {
            tracing::warn!(id = file.meta.id, "主题引用有误：{problem}");
        }
        let theme = Self {
            families: Arc::new(FontFamilies::new(&file.text)),
            sizes: TextSizes::default(),
            animations: true,
            highlight: None,
            surface: None,
            fresh_words: true,
            file: Arc::new(file),
            dark,
            assets: Arc::default(),
        };
        Ok(theme.with_dark(dark))
    }

    /// 读主题目录：`dir/theme.json` 加它用到的图片与字体（路径相对 `dir`）。
    pub fn from_dir(dir: &Path, dark: bool) -> Result<Self, ThemeError> {
        let json = std::fs::read_to_string(dir.join("theme.json"))?;
        let mut theme = Self::from_json(&json, dark)?;
        let inherited = extends::base_id(&json).map_or(&[][..], |id| builtin_files(&id));
        theme.assets = Arc::new(Assets::load(&theme.file, dir, inherited));
        Ok(theme)
    }

    /// 同一主题换外观；主题锁定了外观（`meta.appearance`）时不换。
    pub fn with_dark(&self, dark: bool) -> Self {
        Self {
            file: Arc::clone(&self.file),
            dark: self.locked_dark().unwrap_or(dark),
            assets: Arc::clone(&self.assets),
            families: Arc::clone(&self.families),
            sizes: self.sizes,
            animations: self.animations,
            highlight: self.highlight,
            surface: self.surface,
            fresh_words: self.fresh_words,
        }
    }

    /// 同一主题换上设置里的字号。
    pub fn with_text_sizes(&self, sizes: TextSizes) -> Self {
        Self {
            sizes,
            ..self.clone()
        }
    }

    /// 同一主题换上设置里的候选高亮底色；`None` 用主题的。
    pub fn with_highlight(&self, highlight: Option<Color>) -> Self {
        Self {
            highlight,
            ..self.clone()
        }
    }

    /// 同一主题换上设置里的候选窗口底色；`None` 用主题的。
    pub fn with_surface(&self, surface: Option<Color>) -> Self {
        Self {
            surface,
            ..self.clone()
        }
    }

    /// 同一主题换上设置里的「只给生词上醒目色」开关：关掉后不分生词熟词，所有译文都用醒目色。
    pub fn with_fresh_words(&self, fresh_words: bool) -> Self {
        Self {
            fresh_words,
            ..self.clone()
        }
    }

    /// 是否只给生词上主题的醒目色（`@fresh`）；关掉后不分生词熟词，所有译文都用它。
    pub(crate) fn fresh_words(&self) -> bool {
        self.fresh_words
    }

    /// 同一主题换上设置里的动画开关。
    pub fn with_animations(&self, animations: bool) -> Self {
        Self {
            animations,
            ..self.clone()
        }
    }

    pub(crate) fn animations(&self) -> bool {
        self.animations
    }

    /// 设置里的候选字号相对主题的比例：表格行高与状态条整体跟着它缩放。
    pub(crate) fn candidate_scale(&self) -> f32 {
        self.sizes.ratio("candidate", |name| self.theme_size(name))
    }

    /// 主题自己写的字号（不算设置里的）：设置界面拿它当字号框的初始值。
    pub fn theme_size(&self, name: &str) -> Option<f32> {
        self.families.style(name).map(|spec| spec.size)
    }

    /// 主题 id。
    pub fn id(&self) -> &str {
        &self.file.meta.id
    }

    /// 显示名。
    pub fn name(&self) -> &str {
        &self.file.meta.name
    }

    pub fn author(&self) -> &str {
        &self.file.meta.author
    }

    /// SPDX 许可证标识。
    pub fn license(&self) -> &str {
        &self.file.meta.license
    }

    /// 样式里写到的系统字族名（不含 `system`）。壳按名字查出字体文件，交给 [`crate::Renderer::load_theme_fonts`]。
    pub fn font_families(&self) -> Vec<String> {
        self.families.names()
    }

    /// 随主题带的字体文件（已检查在主题目录里）。
    pub fn font_files(&self) -> &[PathBuf] {
        self.assets.fonts()
    }

    fn default_builtin() -> &'static Theme {
        &builtins()[0]
    }

    /// 锁定的外观：`Some(true)` 只有深色，`Some(false)` 只有浅色。
    pub(crate) fn locked_dark(&self) -> Option<bool> {
        self.file
            .meta
            .appearance
            .map(|appearance| appearance == LockedAppearance::Dark)
    }

    pub(crate) fn file(&self) -> &ThemeFile {
        &self.file
    }

    /// 颜色引用按当前外观取值；设置里的候选高亮底色盖过 `accent`，窗口底色盖过 `surface`
    /// （各内置主题的候选高亮块都填的 `@accent`，窗口与状态条都填的 `@surface`，见 `themes/*/theme.json`）。
    pub(crate) fn color(&self, color: &ColorRef) -> Color {
        match color {
            ColorRef::Literal(color) => *color,
            ColorRef::Variable(name) => match name.as_str() {
                "accent" => self.highlight.unwrap_or_else(|| self.variable(name)),
                "surface" => self.surface.unwrap_or_else(|| self.variable(name)),
                _ => self.variable(name),
            },
        }
    }

    /// 主题文件里某个颜色变量在当前外观下的值；变量不存在时透明。
    fn variable(&self, name: &str) -> Color {
        self.file
            .variables
            .get(name)
            .map_or(Color::rgba(0, 0, 0, 0), |value| value.get(self.dark).0)
    }

    /// 候选列表高亮块当前用的底色：设置里填了就是它，否则是主题的 `accent`。设置界面拿它当占位提示。
    pub fn highlight_color(&self) -> Color {
        self.highlight.unwrap_or_else(|| self.variable("accent"))
    }

    /// 候选窗口当前用的底色：设置里填了就是它，否则是主题的 `surface`。设置界面拿它当占位提示。
    pub fn surface_color(&self) -> Color {
        self.surface.unwrap_or_else(|| self.variable("surface"))
    }

    /// 主题里某个颜色变量在当前外观下的值（设置界面拿它当颜色框的占位提示）。
    pub fn variable_color(&self, name: &str) -> Color {
        self.variable(name)
    }

    /// 主题给某个文字样式单独指定的字族名（`"system"` 表示界面字体）；这一项没写时返回空
    /// （那种样式跟 `text.family`，最终就是界面字体）。设置界面拿它提示「留空用主题字体」到底是什么。
    pub fn style_families(&self, name: &str) -> Vec<String> {
        self.file
            .text
            .styles
            .get(name)
            .and_then(|spec| spec.family.as_ref())
            .map_or_else(Vec::new, |families| families.0.clone())
    }

    /// 主题里写的图片路径对应的位图；没有、没读进来或是 SVG 时为 `None`。
    pub(crate) fn image(&self, path: &str) -> Option<Arc<Pixmap>> {
        self.assets.image(path)
    }

    /// 主题里写的 SVG 图片；没有、没读进来或不是 SVG 时为 `None`。
    pub(crate) fn svg(&self, path: &str) -> Option<Arc<SvgImage>> {
        self.assets.svg(path)
    }

    /// 命名文字样式（点）。
    pub(crate) fn font(&self, name: &str) -> FontSpec {
        let spec = self.families.style(name).unwrap_or(FALLBACK_FONT);
        let spec = text_sizes::scaled(spec, self.sizes.ratio(name, |base| self.theme_size(base)));
        match Role::of_style(name).and_then(|role| self.sizes.weight(role)) {
            Some(weight) => FontSpec { weight, ..spec },
            None => spec,
        }
    }

    /// 拼音行的文字样式：主题给的样式上再盖设置里的拼音行字号与字重（绝对字号，不跟候选字缩放）。
    pub(crate) fn preedit_font(&self, font: &FontRef) -> FontSpec {
        let spec = self.font_ref(font);
        text_sizes::overridden(
            spec,
            self.sizes.size(Role::Preedit),
            self.sizes.preedit_weight,
        )
    }

    /// 设置里给这一处指定的文字颜色；没填返回 `color` 原样。
    pub(crate) fn role_color(&self, role: Role, color: Color) -> Color {
        self.sizes.color(role).unwrap_or(color)
    }

    /// 字族表，渲染器按 [`FamilyId`] 查回退链；两份 `Theme` 共用同一张表时指针相同。
    pub(crate) fn families(&self) -> &Arc<FontFamilies> {
        &self.families
    }

    /// 节点里的 `font`：样式名，或在样式上改字号 / 字重。
    pub(crate) fn font_ref(&self, font: &FontRef) -> FontSpec {
        font.apply(font.base().map_or(FALLBACK_FONT, |name| self.font(name)))
    }

    /// 当前外观下的文字覆盖率 gamma。
    pub(crate) fn text_gamma(&self) -> f32 {
        self.file.text.gamma.get(self.dark)
    }
}

/// 解析好的内置主题，第一次用到时解析。内置主题是随包的，解析失败是 bug。
fn builtins() -> &'static [Theme] {
    static THEMES: OnceLock<Vec<Theme>> = OnceLock::new();
    THEMES.get_or_init(|| {
        BUILTINS
            .iter()
            .map(|(id, source, files)| {
                let mut theme = Theme::from_json(source, false)
                    .unwrap_or_else(|error| panic!("内置主题 {id} 解析失败：{error}"));
                debug_assert_eq!(theme.id(), *id, "内置主题 id 与目录名不一致");
                theme.assets = Arc::new(Assets::embedded(&theme.file, files));
                theme
            })
            .collect()
    })
}

/// 内置主题编进程序的图片；不是内置主题为空。
fn builtin_files(id: &str) -> EmbeddedFiles {
    BUILTINS
        .iter()
        .find(|(builtin, ..)| *builtin == id)
        .map_or(&[], |(.., files)| *files)
}

/// `extends` 按 id 找内置主题的源文件。
fn builtin_source(id: &str) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|(builtin, ..)| *builtin == id)
        .map(|(_, source, _)| *source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_themes_parse_without_problems() {
        for theme in Theme::builtins() {
            assert!(
                validate::problems(theme.file()).is_empty(),
                "{}: {:?}",
                theme.id(),
                validate::problems(theme.file())
            );
        }
        assert_eq!(Theme::builtins().len(), BUILTINS.len());
        let theme = Theme::light();
        assert_eq!(theme.id(), "qingjian");
        assert!(Theme::builtin("wechat", true).is_some());
        assert!(Theme::builtin("nope", false).is_none());
        // 青简绿保持原始配色，护眼绿那套浅灰绿在 eyecare 主题里
        assert_eq!(
            theme.color(&ColorRef::Variable("accent".to_owned())),
            Color::rgba(0xb0, 0xce, 0x7d, 0x7f)
        );
        assert_eq!(
            Theme::dark().color(&ColorRef::Variable("accent".to_owned())),
            Color::rgb(36, 76, 36)
        );
    }

    /// 设置里的候选高亮底色盖过主题的 `accent`，其它变量照旧。
    #[test]
    fn highlight_override_replaces_accent_only() {
        let theme = Theme::light().with_highlight(Some(Color::rgb(0x12, 0x34, 0x56)));
        assert_eq!(
            theme.color(&ColorRef::Variable("accent".to_owned())),
            Color::rgb(0x12, 0x34, 0x56)
        );
        assert_eq!(theme.highlight_color(), Color::rgb(0x12, 0x34, 0x56));
        // 文字色不受影响
        assert_eq!(
            theme.color(&ColorRef::Variable("text".to_owned())),
            Theme::light().color(&ColorRef::Variable("text".to_owned()))
        );
        // 换外观 / 换字号之后覆盖还在（设置没变）
        let dark = theme.with_dark(true);
        assert_eq!(
            dark.color(&ColorRef::Variable("accent".to_owned())),
            Color::rgb(0x12, 0x34, 0x56)
        );
        // 清掉恢复主题色
        let plain = theme.with_highlight(None);
        assert_eq!(
            plain.color(&ColorRef::Variable("accent".to_owned())),
            Color::rgba(0xb0, 0xce, 0x7d, 0x7f)
        );
    }

    #[test]
    fn theme_file_accepts_comments_trailing_commas_and_schema_key() {
        let theme = Theme::from_json(
            r##"{
                "$schema": "https://qingjian.app/schema/theme-1.json",
                "extends": "qingjian", // 以青简绿为底
                "schema": 1,
                "meta": { "id": "jsonc", "name": "注释", },
                /* 只改高亮色 */
                "variables": { "accent": "#ffb7d5", },
            }"##,
            false,
        )
        .unwrap();
        assert_eq!(theme.id(), "jsonc");
        assert_eq!(
            theme.color(&ColorRef::Variable("accent".to_owned())),
            Color::rgb(0xff, 0xb7, 0xd5)
        );
    }

    #[test]
    fn builtin_images_are_all_embedded() {
        for theme in Theme::builtins() {
            for path in assets::image_paths(theme.file()) {
                assert!(
                    theme.image(&path).is_some() || theme.svg(&path).is_some(),
                    "内置主题 {} 用到的 {path} 没有编进程序",
                    theme.id()
                );
            }
        }
    }

    #[test]
    fn user_theme_inherits_builtin_images() {
        let dir = std::env::temp_dir().join("qingjian-theme-inherits-sakura");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("theme.json"),
            r##"{ "extends": "sakura", "schema": 1, "meta": { "id": "my-sakura", "name": "我的樱花" },
                 "variables": { "text": "#000000" } }"##,
        )
        .unwrap();
        let theme = Theme::from_dir(&dir, false).unwrap();
        assert!(theme.image("images/character.png").is_some());
        assert!(theme.svg("images/heart.svg").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_theme_without_extends_builds_on_qingjian() {
        let theme = Theme::from_json(
            r##"{ "schema": 1, "meta": { "id": "red", "name": "红" }, "variables": { "accent": "#ff0000" } }"##,
            false,
        )
        .unwrap();
        assert_eq!(
            theme.color(&ColorRef::Variable("accent".to_owned())),
            Color::rgb(255, 0, 0)
        );
        assert_eq!(theme.font("candidate"), Theme::light().font("candidate"));
    }

    #[test]
    fn text_sizes_scale_styles_and_rows() {
        let theme = Theme::light();
        let (candidate, index, annotation) = (
            theme.font("candidate"),
            theme.font("index"),
            theme.font("annotation"),
        );
        let ratio = 26.0 / candidate.size;
        let sized = theme.with_text_sizes(TextSizes::new(26.0, 0.0));
        assert_eq!(sized.font("candidate").size, 26.0);
        assert_eq!(
            sized.font("candidate").line_height,
            candidate.line_height * ratio
        );
        assert_eq!(sized.font("index").size, index.size * ratio);
        assert_eq!(sized.font("annotation"), annotation);
        assert_eq!(sized.candidate_scale(), ratio);
        // 换外观不丢设置里的字号
        assert_eq!(sized.with_dark(true).font("candidate").size, 26.0);
        // 不是正数当没填
        assert_eq!(TextSizes::new(-1.0, f32::NAN), TextSizes::default());
    }

    /// 护眼绿接在青简绿上：只换窗口底色与选中高亮，没改过的变量跟青简绿一模一样。
    #[test]
    fn eyecare_theme_overrides_surface_and_accent() {
        let theme = Theme::builtin("eyecare", false).expect("内置护眼绿");
        assert_eq!(theme.id(), "eyecare");
        assert_eq!(
            theme.color(&ColorRef::Variable("accent".to_owned())),
            Color::rgb(0xe3, 0xec, 0xe4)
        );
        assert_eq!(theme.surface_color(), Color::rgb(0xf6, 0xf8, 0xf5));
        // 拼音行的字号来自主题的「拼音」样式（青简绿只给了「译文」样式）
        assert_eq!(theme.theme_size("preedit"), Some(18.0));
        // 青简绿已经还原成原始配色，护眼绿这套浅灰绿只存在于 eyecare
        assert_ne!(
            theme.color(&ColorRef::Variable("accent".to_owned())),
            Theme::light().color(&ColorRef::Variable("accent".to_owned()))
        );
        // 没被护眼绿碰过的变量照旧继承
        assert_eq!(
            theme.color(&ColorRef::Variable("text".to_owned())),
            Theme::light().color(&ColorRef::Variable("text".to_owned()))
        );
        // 深色下也是青简绿那套（护眼绿只动浅色）
        assert_eq!(
            Theme::builtin("eyecare", true).unwrap().surface_color(),
            Color::rgb(0x1e, 0x1e, 0x1e)
        );
        // 状态条走节点树、不画投影（没有投影，位图就等于可见的那一条，拖动范围才跟看到的一致）
        use crate::theme::file::ColorSpec;
        use crate::theme::file::node::{FillSpec, NodeKind};
        let status = &theme.file().status;
        let root = status.root.as_ref().expect("护眼绿状态条走节点树");
        let NodeKind::Frame { fill, .. } = &root.kind else {
            panic!("状态条根节点是框");
        };
        let FillSpec::Color(fill) = fill.as_ref().expect("状态条有底色") else {
            panic!("状态条底色是纯色");
        };
        let ColorSpec::Fixed(fill) = fill else {
            panic!("状态条底色没有条件");
        };
        assert_eq!(theme.color(fill), Color::rgb(0xf6, 0xf8, 0xf5));
        assert!(root.layout.effects.as_deref().unwrap_or(&[]).is_empty());
        assert!(status.effects.is_empty());
        // 青简绿自己仍留着投影与灰色格间线
        let base = Theme::light();
        assert!(!base.file().status.effects.is_empty());
        assert_ne!(
            base.color(&base.file().status.normal),
            Color::rgb(0x3a, 0x4b, 0x40)
        );
    }

    /// 设置里的窗口底色盖过主题的 `surface`，其它变量照旧。
    #[test]
    fn surface_override_replaces_surface_only() {
        let theme = Theme::light().with_surface(Some(Color::rgb(0xf6, 0xf8, 0xf5)));
        assert_eq!(
            theme.color(&ColorRef::Variable("surface".to_owned())),
            Color::rgb(0xf6, 0xf8, 0xf5)
        );
        assert_eq!(theme.surface_color(), Color::rgb(0xf6, 0xf8, 0xf5));
        // 高亮色跟主题色都不受影响
        assert_eq!(
            theme.color(&ColorRef::Variable("accent".to_owned())),
            Theme::light().color(&ColorRef::Variable("accent".to_owned()))
        );
        assert_eq!(
            theme.with_dark(true).surface_color(),
            Color::rgb(0xf6, 0xf8, 0xf5)
        );
        // 清掉恢复主题底色
        assert_eq!(
            theme.with_surface(None).surface_color(),
            Color::rgb(0xff, 0xff, 0xff)
        );
    }

    /// 生词醒目标色开关：默认开，关掉不影响主题 `@fresh` 本身的值。
    #[test]
    fn fresh_words_toggle_keeps_theme_variables() {
        let theme = Theme::light();
        assert!(theme.fresh_words());
        let off = theme.with_fresh_words(false);
        assert!(!off.fresh_words());
        assert_eq!(
            off.color(&ColorRef::Variable("fresh".to_owned())),
            Color::rgb(0xff, 0x8d, 0x28)
        );
        // 换外观不丢开关
        assert!(!off.with_dark(true).fresh_words());
    }

    /// 主题给自己指定了字族的样式，设置界面拿它提示「留空用主题字体」到底是什么。
    #[test]
    fn style_families_read_from_theme() {
        let sakura = Theme::builtin("sakura", false).unwrap();
        assert_eq!(
            sakura.style_families("candidate"),
            ["Songti SC", "SimSun", "system"]
        );
        // 没写 family 的样式返回空（跟 `text.family` / 界面字体走）
        assert!(Theme::light().style_families("candidate").is_empty());
    }
}
