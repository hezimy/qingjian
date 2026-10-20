//! 候选窗口的定位锚点：组句起点 / 选区在屏幕上的矩形，拿不到时退到鼠标位置。
//!
//! 微信 4.x 对「非空范围」的 GetTextExt 只回终点处 1 像素残片（首键却给整行高，实测
//! (…,1062,…,1082) 对 (…,1063,…,1064)），锚点因此逐键跳、贴下方还盖住输入行——组句
//! 一律折叠到起点再量（对齐 weasel），残片按上次的正常行高补回整行。
//!
//! WPS 文字（wps.exe）对组句范围**每个键**都报窗口右下角逐个像素（实测整个会话
//! (2559,1399,2560,1399) 一动不动，候选窗因此死在屏幕右下角）。这种值在 [`stabilize`]
//! 里一律不采用，退到次锚点——系统光标（[`system_caret_rect`]，跟着真光标走），再不行才是鼠标。
//!
//! 但 `GetSelection` 量出来的**也是同一个角落残片**（实测整个会话
//! `候选锚点 来源=起点 rect=(2559,1399,2560,1399)`，一条「改用」日志都没有）：WPS 只有
//! 「只在候选窗口」模式，没有组句范围，`stabilize` 的主锚点就是 [`caret_rect`] 量出来的
//! 选区——[`caret_rect`] 原先信它，系统光标那条更可靠的路就永远轮不到，等于没有次锚点。
//! 于是角落残片被当「上一次的正常位置」记进 `last_anchor`，之后每一键都退不出角落。所以
//! 贴角的退化残片在 [`caret_rect`] 里一样不算数，且补整行前要先认出来（见 `repair_degenerate`）。
//!
//! 判「贴角」不只靠前台窗口：WPS 里 DLL 看到的 `GetForegroundWindow` 未必是输入焦点那个
//! 窗口，拿不到就整条判定失效、退化残片被原样采用（`stabilize` 的「改用」日志一条也没有，
//! 候选窗照样死在屏幕右下角）。所以退化残片（宽 ≤2 且高 ≤2，正是 WPS 的 1×0）贴**显示器
//! 工作区**右下角也算贴角，与窗口矩形无关。

use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicI32, Ordering};

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO, MapWindowPoints,
    MonitorFromPoint,
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
/// 没有组句范围可量，就用它给候选窗口定位。
///
/// 选区量出来的**退化残片贴在工作区右下角时不算数**：WPS 的 `GetSelection` 不空，量出来
/// 和组句范围报的是同一个假光标（框架头注释里的实测），采信它 [`stabilize`] 退来退去还是
/// 那个角落——等于没有次锚点。放它过去，继续退到系统光标（跟着真光标走）、再退鼠标。
pub(crate) fn caret_rect(context: &ITfContext, ec: u32) -> ScreenRect {
    let selected = selection_range(context, ec).map(|range| anchor_rect(context, ec, &range));
    if let Some(rect) = selected.filter(|rect| usable_caret(*rect)) {
        return rect;
    }
    match system_caret_rect() {
        Some(rect) => {
            crate::com::log::log(&format!(
                "候选锚点 次选=系统光标 rect=({},{},{},{})",
                rect.left, rect.top, rect.right, rect.bottom
            ));
            rect
        }
        None => mouse_screen_rect(),
    }
}

/// 选区量出来的锚点能不能当插入点用。WPS 的 `GetSelection` 量出来贴着工作区右下角，和组句
/// 范围报的是同一个退化残片，采信它 [`stabilize`] 就退不出角落（等于没有次锚点，见 [`caret_rect`]）。
/// 单独拎成函数是为了让回归测试能直接跑这条判定：真跑 [`caret_rect`] 要 TSF 上下文。
fn usable_caret(rect: ScreenRect) -> bool {
    !cornerish(rect, None)
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
    // WPS 报的假光标是贴在显示器**工作区**右下角的退化残片（宽 ≤2 高 ≤2，正是它的 1×0）。
    // 按上次的行高补整行，它就成了普通的一行高光标，贴角判定（`cornerish_at_work_area` 只认
    // 退化矩形）再也认不出来，残片被原样采用、候选窗死在右下角——所以这类残片一律不补。
    if cornerish(to_screen(*rect), None) {
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
    if rect.right <= rect.left && rect.bottom <= rect.top {
        return None;
    }
    // 不实现真文本 store 的应用（部分全屏游戏）GetTextExt 会「成功」返回屏幕外假矩形
    // （Dota2 进程里实测拿到 (-1000,-1000,…)）。这种锚点会把候选窗钉到错误的屏幕角落，
    // 一律视为拿不到，退回鼠标位置——游戏里跟随鼠标总比固定在屏幕外好。
    if !rect_on_real_monitor(&rect) {
        crate::com::log::log(&format!(
            "GetTextExt 返回屏幕外矩形 ({},{},{},{})，退回鼠标定位",
            rect.left, rect.top, rect.right, rect.bottom
        ));
        return None;
    }
    Some(rect)
}

/// 矩形是否落在真实显示器上：以中心点命中哪块屏判定。多屏副屏负坐标是合法的，
/// 只有完全在所有显示器之外（如 (-1000,-1000) 这类假 store 返回值）才判无效。
fn rect_on_real_monitor(rect: &RECT) -> bool {
    let center = POINT {
        x: rect.left + (rect.right - rect.left) / 2,
        y: rect.top + (rect.bottom - rect.top) / 2,
    };
    unsafe { !MonitorFromPoint(center, MONITOR_DEFAULTTONULL).is_invalid() }
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

    /// WPS 报的假光标形状：显示器工作区右下角那个 1×0 残片（实测 (2559,1399,2560,1399)，
    /// 工作区右下角 (2560,1400)）。工作区取自本机真实显示器，换机器这条判据也成立。
    fn work_area_corner_fragment() -> ScreenRect {
        let work = to_screen(work_area_near(POINT { x: 0, y: 0 }));
        screen(work.right - 1, work.bottom - 1, work.right, work.bottom - 1)
    }

    // 回归（WPS 实测，2026-10-19 日志：整个会话 `来源=起点 rect=(2559,1399,2560,1399)`、
    // 一条「改用」都没有）：`GetSelection` 不空，量出来和组句范围报的是同一个角落残片。
    // 这种值当插入点采信，`caret_rect` 就把系统光标那条更可靠的路挡掉了——等于没有次锚点，
    // `stabilize` 退来退去还是角落。所以它必须被 `usable_caret` 判掉，退到系统光标 / 鼠标。
    #[test]
    fn wps_corner_fragment_is_not_usable_as_the_insertion_point() {
        let fragment = work_area_corner_fragment();
        assert!(cornerish(fragment, None)); // 真实工作区，不靠窗口矩形
        assert!(!usable_caret(fragment));
        assert!(usable_caret(caret_at(430, 390))); // 正常的一行高光标照用
    }

    // 回归：角落残片不许补整行——补了就是一行高的普通光标，贴角判定（只认宽 ≤2 高 ≤2）
    // 再也认不出来，残片被原样采用、候选窗死在右下角。微信那种不在角落的残片照旧补。
    #[test]
    fn a_corner_fragment_is_never_repaired() {
        let _serial = serial();
        let fragment = work_area_corner_fragment();
        let mut raw = RECT {
            left: fragment.left,
            top: fragment.top,
            right: fragment.right,
            bottom: fragment.bottom,
        };
        LAST_LINE_HEIGHT.store(0, Ordering::Relaxed);
        let mut first = rect(1062, 1082); // 先记下一次正常行高（微信首键形状）
        assert!(!repair_degenerate(&mut first));
        assert!(!repair_degenerate(&mut raw)); // 角落残片不补
        assert_eq!(raw.bottom - raw.top, 0);
        LAST_LINE_HEIGHT.store(0, Ordering::Relaxed);
    }

    // 回归（WPS 实测形状）：全在「只在候选窗口」模式，没有组句范围，窗口矩形也是 `None`
    // （`GetForegroundWindow` 拿不到输入焦点那个窗口）。没修时主锚点就是角落残片、次锚点无、
    // 上一次无，`stabilize` 只好采用角落——候选窗死在右下角；修好后 `caret_rect` 不采信残片，
    // 交给系统光标 / 鼠标，`stabilize` 也就跟着真光标走。
    #[test]
    fn wps_shape_without_a_window_no_longer_dead_ends_in_the_corner() {
        let corner = work_area_corner_fragment();
        let caret = caret_at(430, 390);
        assert_eq!(stabilize(None, corner, None, None), corner); // 修前的死路
        assert!(!usable_caret(corner));
        assert_eq!(stabilize(None, caret, None, None), caret); // 修后跟着真光标
    }
}
