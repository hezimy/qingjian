//! 候选窗口的定位锚点：组句起点 / 选区在屏幕上的矩形，拿不到时退到鼠标位置。
//!
//! 微信 4.x 对「非空范围」的 GetTextExt 只回终点处 1 像素残片（首键却给整行高，实测
//! (…,1062,…,1082) 对 (…,1063,…,1064)），锚点因此逐键跳、贴下方还盖住输入行——组句
//! 一律折叠到起点再量（对齐 weasel），残片按上次的正常行高补回整行。
//!
//! WPS 文字（wps.exe）对组句范围**每个键**都报窗口右下角逐个像素（实测整个会话
//! (2559,1399,2560,1399) 一动不动，候选窗因此死在屏幕右下角），`GetSelection` 也是空的、
//! 连选区都没得量。这种值在 [`stabilize`] 里一律不采用，退到次锚点——系统光标
//! （[`system_caret_rect`]，跟着真光标走），再不行才是鼠标。
//!
//! 判「贴角」不只靠前台窗口：WPS 里 DLL 看到的 `GetForegroundWindow` 未必是输入焦点那个
//! 窗口，拿不到就整条判定失效、退化残片被原样采用（`stabilize` 的「改用」日志一条也没有，
//! 候选窗照样死在屏幕右下角）。所以退化残片（宽 ≤2 且高 ≤2，正是 WPS 的 1×0）贴**显示器
//! 工作区**右下角也算贴角，与窗口矩形无关。

use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicI32, Ordering};

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MapWindowPoints, MonitorFromPoint,
};
use windows::Win32::UI::TextServices::{
    ITfContext, ITfRange, TF_ANCHOR_START, TF_DEFAULT_SELECTION, TF_SELECTION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo, GetWindowRect,
};
use windows::core::BOOL;

use qingjian_platform::protocol::ScreenRect;

/// `range` 的屏幕矩形，拿不到（有些应用给全零 / 空矩形）退到鼠标处；残片矩形补回整行。
/// 每次取锚都留一行日志：候选框「上下乱跳」的排查全靠它（微信首键矩形坏掉一类的应用怪癖要有数据才能定）。
pub(crate) fn anchor_rect(context: &ITfContext, ec: u32, range: &ITfRange) -> ScreenRect {
    let (mut rect, source) = collapsed_rect(context, ec, range)
        .map(|rect| (rect, "起点"))
        .or_else(|| range_rect(context, ec, range).map(|rect| (rect, "整段")))
        .unwrap_or_else(|| (mouse_anchor(), "鼠标回退"));
    let repaired = repair_degenerate(&mut rect);
    crate::com::log::log(&format!(
        "候选锚点 来源={source} rect=({},{},{},{}){}",
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        if repaired {
            "（残片已补整行）"
        } else {
            ""
        },
    ));
    to_screen(rect)
}

/// 组句**起点**的矩形：克隆范围折叠到起点再量。组句期间起点不动，锚点逐键稳定；
/// 微信对空范围（光标点）的 GetTextExt 正常，正好绕开非空范围的残片实现（小狼毫
/// Composition.cpp 同款策略）。克隆再折叠，不动组句本体的范围。
fn collapsed_rect(context: &ITfContext, ec: u32, range: &ITfRange) -> Option<RECT> {
    let clone = unsafe { range.Clone() }.ok()?;
    unsafe { clone.Collapse(ec, TF_ANCHOR_START) }.ok()?;
    range_rect(context, ec, &clone)
}

/// 插入点（没有选区时是光标，有选区时是选区）的屏幕矩形。「只在候选窗口」模式应用里不放 marked text，
/// 没有组句范围可量，就用它给候选窗口定位。WPS 的 `GetSelection` 返回空，退到系统光标（见下）。
pub(crate) fn caret_rect(context: &ITfContext, ec: u32) -> ScreenRect {
    match selection_range(context, ec) {
        Some(range) => anchor_rect(context, ec, &range),
        None => match system_caret_rect() {
            Some(rect) => {
                crate::com::log::log(&format!(
                    "候选锚点 次选=系统光标 rect=({},{},{},{})",
                    rect.left, rect.top, rect.right, rect.bottom
                ));
                rect
            }
            None => mouse_screen_rect(),
        },
    }
}

/// 系统光标（`GetGUIThreadInfo` 的 `rcCaret`）的屏幕矩形：TSF 选区量不到时的真光标位置。
/// 线程 id 传 0 就是「前台窗口所在的线程」（Old New Thing 同款用法），省得自己取。
/// 宿主自绘光标（不 `CreateCaret`）或跨权限（前台是提权窗口）拿不到时是空矩形 / 调用失败，返回 `None`。
///
/// `rcCaret` 是 `hwndCaret` 的**客户区坐标**，要 `MapWindowPoints` 转屏幕坐标再用。
fn system_caret_rect() -> Option<ScreenRect> {
    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(0, &mut info) }.ok()?;
    if info.hwndCaret.is_invalid() {
        return None;
    }
    let caret = info.rcCaret;
    if caret.right <= caret.left && caret.bottom <= caret.top {
        return None; // 没有系统光标：自绘光标的应用
    }
    let mut points = [
        POINT {
            x: caret.left,
            y: caret.top,
        },
        POINT {
            x: caret.right,
            y: caret.bottom,
        },
    ];
    // hwndto 传 None：转成屏幕坐标
    unsafe { MapWindowPoints(Some(info.hwndCaret), None, &mut points) };
    Some(to_screen(RECT {
        left: points[0].x,
        top: points[0].y,
        right: points[1].x,
        bottom: points[1].y,
    }))
}

/// 当前选区的范围；`GetSelection` 移交所有权，由调用方释放。
pub(crate) fn selection_range(context: &ITfContext, ec: u32) -> Option<ITfRange> {
    let mut selection = [TF_SELECTION::default()];
    let mut fetched = 0u32;
    unsafe {
        context
            .GetSelection(ec, TF_DEFAULT_SELECTION, &mut selection, &mut fetched)
            .ok()?;
    }
    if fetched == 0 {
        return None;
    }
    unsafe { ManuallyDrop::take(&mut selection[0].range) }
}

/// 没有可量的范围时的锚点：鼠标处一个零宽、约一行高的矩形。
pub(crate) fn mouse_screen_rect() -> ScreenRect {
    to_screen(mouse_anchor())
}

/// 上一条正常锚点矩形量到的行高（物理像素），见 `repair_degenerate`。
static LAST_LINE_HEIGHT: AtomicI32 = AtomicI32::new(0);

/// 行高的可信区间：低于下限的是光标残片，高于上限的多半是整个编辑框，都不能当「一行」记。
const MIN_LINE_HEIGHT: i32 = 8;
const MAX_LINE_HEIGHT: i32 = 200;

/// 残片矩形按「顶边＝行顶、行高＝上次的正常行高」补回整行：同一行内摆放不再翻转，
/// 残片底边也不会把窗口顶进输入行。返回是否补过。
fn repair_degenerate(rect: &mut RECT) -> bool {
    let height = rect.bottom - rect.top;
    if (MIN_LINE_HEIGHT..=MAX_LINE_HEIGHT).contains(&height) {
        LAST_LINE_HEIGHT.store(height, Ordering::Relaxed);
        return false;
    }
    let remembered = LAST_LINE_HEIGHT.load(Ordering::Relaxed);
    if height < MIN_LINE_HEIGHT && remembered >= MIN_LINE_HEIGHT {
        rect.bottom = rect.top + remembered;
        return true;
    }
    false
}

fn range_rect(context: &ITfContext, ec: u32, range: &ITfRange) -> Option<RECT> {
    let mut rect = RECT::default();
    let mut clipped = BOOL(0);
    unsafe {
        let view = context.GetActiveView().ok()?;
        view.GetTextExt(ec, range, &mut rect, &mut clipped).ok()?;
    }
    (rect.right > rect.left || rect.bottom > rect.top).then_some(rect)
}

fn mouse_anchor() -> RECT {
    let mut point = POINT::default();
    let _ = unsafe { GetCursorPos(&mut point) };
    RECT {
        left: point.x,
        top: point.y,
        right: point.x,
        bottom: point.y + 16,
    }
}

fn to_screen(rect: RECT) -> ScreenRect {
    ScreenRect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

/// 前台窗口的屏幕矩形。用来认出「光标被报成窗口右下角」这种没排好版的结果。
pub(crate) fn foreground_bounds() -> Option<ScreenRect> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == HWND::default() {
        return None;
    }
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
    Some(to_screen(rect))
}

/// 组句范围和选区经常一个准、一个落到窗口右下角。离上一次太远时取更近的那个；
/// 两个都贴在大窗口右下角、上一次还在输入框附近，就留在上一次。
///
/// 主锚点贴在大窗口右下角逐个像素时（WPS 每个键都这样，Chromium 部分上屏后的首键也这样）
/// **一律不采用**：有新鲜的次锚点（系统光标 / 选区）就用它，没有就留在上一次的正常位置，
/// 两样都没有才只好用它——候选窗死在屏幕角落里，比贴偏一点糟得多。
pub(crate) fn stabilize(
    previous: Option<ScreenRect>,
    primary: ScreenRect,
    secondary: Option<ScreenRect>,
    window: Option<ScreenRect>,
) -> ScreenRect {
    if cornerish(primary, window) {
        if let Some(secondary) = secondary
            && !cornerish(secondary, window)
        {
            return secondary;
        }
        if let Some(previous) = previous
            && !cornerish(previous, window)
        {
            return previous;
        }
        return primary;
    }
    let mut chosen = primary;
    if let (Some(previous), Some(secondary)) = (previous, secondary) {
        let primary_jump = jump(previous, primary);
        let secondary_jump = jump(previous, secondary);
        if primary_jump > 160 && secondary_jump + 80 < primary_jump {
            chosen = secondary;
        }
    }
    if let Some(previous) = previous
        && jump(previous, chosen) > 160
        && cornerish(chosen, window)
        && !cornerish(previous, window)
    {
        return previous;
    }
    chosen
}

fn jump(from: ScreenRect, to: ScreenRect) -> i32 {
    (from.left - to.left).abs() + (from.top - to.top).abs()
}

/// 大窗口右下角一小块。WPS 对组句范围恒定报这个位置，部分上屏后 Chromium 系浏览器也拿它
/// 顶替还没排好的光标。两条独立判定取或：显示器工作区右下角的退化残片，或前台窗口右下角
/// 的小矩形。任一条拿不到就退到另一条，不互相拖累。
fn cornerish(rect: ScreenRect, window: Option<ScreenRect>) -> bool {
    let work = work_area_near(POINT {
        x: rect.left,
        y: rect.top,
    });
    cornerish_at_work_area(rect, to_screen(work)) || cornerish_at_window(rect, window)
}

/// 退化残片贴在工作区右下角：WPS 报的假光标恒定落在那儿。**只认退化矩形**（宽 ≤2 且高 ≤2，
/// 正是它的 1×0），真正的一行高光标（宽 1 高 18）不算——那种贴角也可能真的在屏幕右下角
/// 打字，别把正常锚点误判掉。工作区右下角是任务栏上沿那个角。
fn cornerish_at_work_area(rect: ScreenRect, work: ScreenRect) -> bool {
    if rect.right - rect.left > 2 || rect.bottom - rect.top > 2 {
        return false;
    }
    let dx = work.right - rect.left;
    let dy = work.bottom - rect.top;
    (0..220).contains(&dx) && (0..220).contains(&dy)
}

fn cornerish_at_window(rect: ScreenRect, window: Option<ScreenRect>) -> bool {
    let Some(window) = window else {
        return false;
    };
    let width = window.right - window.left;
    let height = window.bottom - window.top;
    if width < 500 || height < 400 {
        return false;
    }
    let dx = window.right - rect.left;
    let dy = window.bottom - rect.top;
    (0..220).contains(&dx)
        && (0..220).contains(&dy)
        && rect.right - rect.left < 500
        && rect.bottom - rect.top < 120
}

/// `point` 所在（最近）显示器的工作区（屏幕减去任务栏）；拿不到时给个大范围，让贴角判定
/// 自然落空。
fn work_area_near(point: POINT) -> RECT {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let found = unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut info).as_bool()
    };
    if found {
        info.rcWork
    } else {
        RECT {
            left: 0,
            top: 0,
            right: i32::MAX,
            bottom: i32::MAX,
        }
    }
}

/// 日志里描述一个可选矩形：`(l,t,r,b)` 或 `无`。
pub(crate) fn describe(rect: Option<ScreenRect>) -> String {
    match rect {
        Some(rect) => format!(
            "({},{},{},{})",
            rect.left, rect.top, rect.right, rect.bottom
        ),
        None => "无".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    /// 各测试都读写全局的 `LAST_LINE_HEIGHT`，并行跑会互相踩，拿这把锁排队。
    fn serial() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn rect(top: i32, bottom: i32) -> RECT {
        RECT {
            left: 100,
            top,
            right: 103,
            bottom,
        }
    }

    fn screen(left: i32, top: i32, right: i32, bottom: i32) -> ScreenRect {
        ScreenRect {
            left,
            top,
            right,
            bottom,
        }
    }

    // 微信输入框实测形状：首键整行高 20，之后每键 1 像素残片；补回后同一行底边一致，
    // 摆放判定不再翻转。
    #[test]
    fn fragment_is_repaired_to_last_line_height() {
        let _serial = serial();
        LAST_LINE_HEIGHT.store(0, Ordering::Relaxed);
        let mut orphan = rect(1063, 1064);
        assert!(!repair_degenerate(&mut orphan)); // 还没记过行高，照用残片
        assert_eq!(orphan.bottom, 1064);

        let mut first = rect(1062, 1082);
        assert!(!repair_degenerate(&mut first)); // 整行高，记下不补
        assert_eq!(first.bottom, 1082);

        let mut later = rect(1063, 1064);
        assert!(repair_degenerate(&mut later)); // 同行的残片补回整行
        assert_eq!(later.bottom, 1083);
    }

    // 整框矩形（整个编辑框那么高）不能当行高记，也不许被补。
    #[test]
    fn oversized_rect_is_neither_stored_nor_repaired() {
        let _serial = serial();
        LAST_LINE_HEIGHT.store(0, Ordering::Relaxed);
        let mut whole = rect(0, 900);
        assert!(!repair_degenerate(&mut whole));
        assert_eq!(whole.bottom, 900);
        assert_eq!(LAST_LINE_HEIGHT.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn keeps_the_caret_when_the_composition_rect_jumps_to_the_corner() {
        let window = screen(0, 0, 1600, 900);
        let previous = screen(40, 80, 40, 100);
        let corner = screen(1500, 860, 1500, 880);
        let caret = screen(56, 80, 56, 100);
        assert_eq!(
            stabilize(Some(previous), corner, Some(caret), Some(window)),
            caret
        );
        assert_eq!(
            stabilize(Some(previous), corner, Some(corner), Some(window)),
            previous
        );
    }

    #[test]
    fn follows_a_real_move() {
        let window = screen(0, 0, 1600, 900);
        let previous = screen(40, 80, 40, 100);
        let moved = screen(40, 120, 40, 140);
        assert_eq!(
            stabilize(Some(previous), moved, Some(moved), Some(window)),
            moved
        );
        let corner = screen(1500, 860, 1502, 880);
        assert_eq!(stabilize(None, corner, None, Some(window)), corner);
    }

    // WPS 实测形状：组句**第一键**就把锚点报成窗口右下角逐个像素（整个会话都这样），
    // 没有上一次的正常位置可留——退到次锚点（系统光标 / 鼠标：刚点进输入框，约等于光标）。
    #[test]
    fn falls_back_to_the_caret_when_the_first_rect_is_the_corner() {
        let window = screen(0, 0, 2560, 1400);
        let corner = screen(2559, 1399, 2560, 1399);
        let caret = screen(430, 390, 430, 412);
        assert_eq!(stabilize(None, corner, Some(caret), Some(window)), caret);
        // 连次锚点都贴在角落里（鼠标就在右下角）：只好用主锚点，别把上次的拽回来
        assert_eq!(stabilize(None, corner, Some(corner), Some(window)), corner);
    }

    // 上一段组句也停在角落（WPS 里连着打两段）：次锚点还是新鲜的光标位置，照用。
    #[test]
    fn follows_the_caret_when_the_previous_anchor_is_also_the_corner() {
        let window = screen(0, 0, 2560, 1400);
        let corner = screen(2559, 1399, 2560, 1399);
        let caret = screen(430, 390, 430, 412);
        assert_eq!(
            stabilize(Some(corner), corner, Some(caret), Some(window)),
            caret
        );
    }

    // 主锚点正常时新逻辑不插手：该跟还是跟，该留还是留（老行为原样）。
    #[test]
    fn a_sane_primary_is_untouched() {
        let window = screen(0, 0, 1600, 900);
        let previous = screen(40, 80, 40, 100);
        let moved = screen(200, 80, 200, 100);
        assert_eq!(
            stabilize(
                Some(previous),
                moved,
                Some(caret_at(900, 800)),
                Some(window)
            ),
            moved
        );
    }

    fn caret_at(left: i32, top: i32) -> ScreenRect {
        screen(left, top, left + 1, top + 18)
    }

    // WPS 里 `GetForegroundWindow` 拿不到、窗口矩形为 `None`，判定不能因此失效：退化残片
    // （1×0）贴工作区右下角照样算贴角，退到系统光标。
    #[test]
    fn work_area_corner_catches_the_wps_fragment_without_a_window() {
        let work = screen(0, 0, 2560, 1400);
        let fragment = screen(2559, 1399, 2560, 1399);
        assert!(cornerish_at_work_area(fragment, work));
    }

    // 真正的一行高光标（宽 1 高 18）不是残片：即便恰好在工作区右下角也原样采用，
    // 别把「用户就是在屏幕右下角打字」的锚点误判成贴角。
    #[test]
    fn work_area_corner_spares_a_real_caret_row() {
        let work = screen(0, 0, 2560, 1400);
        let caret = screen(2559, 1399, 2560, 1417);
        assert!(!cornerish_at_work_area(caret, work));
    }

    #[test]
    fn a_fragment_in_the_middle_is_not_the_corner() {
        let work = screen(0, 0, 2560, 1400);
        let fragment = screen(430, 390, 431, 390);
        assert!(!cornerish_at_work_area(fragment, work));
    }
}
