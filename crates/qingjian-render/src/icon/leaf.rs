//! 状态条设置那一格的树叶：叶形轮廓 + 中间主脉 + 几对分叉的侧脉，只描边不填充。
//! 自己画是为了不受字体回退影响：字形里排在前面的 Segoe UI Emoji 会把符号画成彩色。

use bamboo_slip::tiny_skia::{Path, PathBuilder};

use bamboo_slip::{Canvas, Color, Icon};

/// 轮廓描边宽度相对边长的比例：比侧脉粗一点，小尺寸下轮廓先站得住。
const OUTLINE_RATIO: f32 = 0.085;

/// 侧脉描边宽度相对边长的比例：比轮廓细，看得清主次。
const VEIN_RATIO: f32 = 0.065;

/// 叶形半边从叶柄到叶尖的采样段数：逐段折线描出来就是弧线。
const SEGMENTS: usize = 24;

/// 叶柄、叶尖在方框里的纵向位置（0 顶、1 底）与最宽处的半宽。
const BASE: f32 = 0.95;
const TIP: f32 = 0.06;
const WIDTH: f32 = 0.36;

/// 主脉在叶柄、叶尖两头留出的余量。
const MIDRIB: (f32, f32) = (0.04, 0.96);

/// 侧脉起点（0 叶柄、1 叶尖，三对自下而上）、自起点往叶尖升多少、末端落在叶缘的比例。
/// 三对都排在叶形较宽的下半段，靠叶尖那一头留空；末端接到叶缘上，小尺寸下才是一整条而不是一点灰。
const VEIN_STARTS: [f32; 3] = [0.26, 0.42, 0.58];
const VEIN_RISE: f32 = 0.13;
const VEIN_REACH: f32 = 0.95;

#[derive(Debug)]
pub(super) struct Leaf;

impl Icon for Leaf {
    fn draw(&self, canvas: &mut Canvas, x: f32, y: f32, size: f32, color: Color) {
        draw_leaf(canvas, x, y, size, color);
    }
}

fn draw_leaf(canvas: &mut Canvas, x: f32, y: f32, size: f32, color: Color) {
    // 用描字形轮廓那一套：圆头端点与接头，叶柄、叶尖不会被拉出长刺
    if let Some(outline) = leaf_outline(x, y, size) {
        canvas.stroke_outline(&outline, (size * OUTLINE_RATIO).max(1.0), color);
    }
    if let Some(veins) = leaf_veins(x, y, size) {
        canvas.stroke_outline(&veins, (size * VEIN_RATIO).max(1.0), color);
    }
}

/// 叶形轮廓：左右两条对称的曲线，在叶柄与叶尖收成尖。
fn leaf_outline(x: f32, y: f32, size: f32) -> Option<Path> {
    let mut path = PathBuilder::new();
    for t in steps() {
        let (px, py) = leaf_point(x, y, size, t, -1.0);
        if t == 0.0 {
            path.move_to(px, py);
        } else {
            path.line_to(px, py);
        }
    }
    for t in steps().rev() {
        let (px, py) = leaf_point(x, y, size, t, 1.0);
        path.line_to(px, py);
    }
    path.close();
    path.finish()
}

/// 脉络：一条主脉贯通叶柄到叶尖，侧脉从主脉斜着分到叶缘。
fn leaf_veins(x: f32, y: f32, size: f32) -> Option<Path> {
    let mut path = PathBuilder::new();
    let (bx, by) = leaf_point(x, y, size, MIDRIB.0, 0.0);
    let (tx, ty) = leaf_point(x, y, size, MIDRIB.1, 0.0);
    path.move_to(bx, by);
    path.line_to(tx, ty);
    for start in VEIN_STARTS {
        for side in [-1.0, 1.0] {
            let (sx, sy) = leaf_point(x, y, size, start, 0.0);
            let (ex, ey) = leaf_point(x, y, size, start + VEIN_RISE, side * VEIN_REACH);
            path.move_to(sx, sy);
            path.line_to(ex, ey);
        }
    }
    path.finish()
}

/// 叶形上的一点：`t` 从叶柄 0 到叶尖 1，`side` 给 ±1 分别取右、左半边，0 落在中轴上。
fn leaf_point(x: f32, y: f32, size: f32, t: f32, side: f32) -> (f32, f32) {
    let px = x + size * (0.5 + side * half_width(t));
    let py = y + size * (BASE + (TIP - BASE) * t);
    (px, py)
}

/// 半宽（相对边长）：`4u(1-u)` 在两头归零、中间鼓起，两头于是收成尖。
/// `t` 先按 0.8 次方拉伸，最宽处落在四成上下，是叶子而不是对称的橄榄。
fn half_width(t: f32) -> f32 {
    let u = t.powf(0.8);
    WIDTH * 4.0 * u * (1.0 - u)
}

/// 0 到 1 的等分点，正反各走一遍就是两条对称的边。
fn steps() -> impl DoubleEndedIterator<Item = f32> {
    (0..=SEGMENTS).map(|i| i as f32 / SEGMENTS as f32)
}
