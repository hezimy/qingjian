//! 候选窗口：不抢焦点、置顶的分层窗口，跟随光标，画拼音行与候选列表，四周柔和阴影。
//! 缺省交给青简渲染器出位图再贴（[`super::painter`]），配置 `renderer = "system"` 时走 GDI：绘制在 [`view`]，
//! 配色 / 字体在 [`theme`]。绘制内容在 [`RenderData`]，一行的展示形态在 [`row`]。设计语言对齐 macOS 端。
//!
//! 窗口显示期间还挂着一条**前台看门狗**（[`CandidateWindow::watch`]）：切到别的应用时 DLL 未必报得回来
//! （切成一个没有输入框的应用，那条焦点通知根本不来），窗口就会一直置顶盖着别人的界面。这条定时器由
//! Server 自己盯前台窗口，换线程了就地收窗，不指望 DLL 说话。

mod click;
mod render_data;
pub(crate) mod row;
pub(crate) mod theme;
pub(crate) mod view;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::Foundation::{E_INVALIDARG, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, GetWindowThreadProcessId,
    IDC_ARROW, IsWindowVisible, KillTimer, LoadCursorW, MSG, SW_HIDE, SW_SHOWNA, SetTimer,
    ShowWindow, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_TIMER, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, PCWSTR, Result, w};

use qingjian_platform::Appearance;
use qingjian_platform::protocol::Frame;
use qingjian_render::Mode;

pub use self::click::CandidateClicks;
pub(crate) use self::render_data::RenderData;
use self::theme::Theme;
use super::layered::{self, Layered};
use super::monitor;
use super::painter::SharedPainter;
use super::window_class::WindowClass;

const CLASS_NAME: PCWSTR = w!("QingjianCandidateWindow");
static CLASS: WindowClass = WindowClass::new();

/// 动画定时器的 id（间隔按渲染器给的 `next_frame`；系统定时器精度约 15.6 ms）。
const ANIMATION_TIMER: usize = 1;

/// 前台看门狗定时器的 id（见文件头）。同一个窗口上的第二个定时器。
const WATCHDOG_TIMER: usize = 2;

/// 看门狗的间隔（毫秒）：切窗口后最多这么久窗口就收掉。够快看不出残留，也不至于白费 CPU。
const WATCHDOG_INTERVAL: u32 = 250;

/// 光标行与候选窗之间的间隙（逻辑像素）。
const CARET_GAP: i32 = 2;

/// 按外观模式解析深浅；`System` 读系统主题。
pub(super) fn resolve_dark(mode: Appearance) -> bool {
    match mode {
        Appearance::Light => false,
        Appearance::Dark => true,
        Appearance::System => system_prefers_dark(),
    }
}

/// `HKCU\...\Themes\Personalize\AppsUseLightTheme` 为 0 是深色；读不到当浅色。
fn system_prefers_dark() -> bool {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("AppsUseLightTheme"))
        .is_ok_and(|value| value == 0)
}

/// 前台窗口所在线程；没有前台窗口（切桌面 / 安全桌面）或问不出来时 `None`。
///
/// 看门狗比的是**线程**而不是窗口句柄：同一个应用弹出个下拉、菜单、工具提示，窗口句柄换了但线程没换，
/// 不该把候选窗收掉；换应用一般就是换线程（换进程）。
fn foreground_thread() -> Option<u32> {
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return None;
    }
    let thread = unsafe { GetWindowThreadProcessId(window, None) };
    (thread != 0).then_some(thread)
}

/// 候选窗口。内容经 `UpdateLayeredWindow` 一次贴上，窗口过程只走默认处理。
pub(crate) struct CandidateWindow {
    hwnd: HWND,

    /// 绘制内容。
    data: RefCell<RenderData>,

    /// 上次用的 DPI，变了重建字体。
    dpi: Cell<u32>,

    /// 上次解析出的深浅，变了重建配色。
    dark: Cell<bool>,

    /// 上次记进日志的缩放值（窗口 DPI、光标所在显示器 DPI）：变了才再记一条（#146）。
    logged_dpi: Cell<Option<(u32, Option<u32>)>>,

    /// 青简渲染器；`None` 走 GDI。
    painter: SharedPainter,

    /// 最近一次贴图时窗口左上角的屏幕坐标；动画帧贴在同一处。
    position: Cell<(i32, i32)>,

    /// 最近一次 `show` 用的光标矩形；设置改了要按新主题就地重画时用它。
    anchor: Cell<Option<RECT>>,

    /// 显示那一刻前台窗口所在线程（= 正在打字的那个应用）。前台换线程了就收窗，见 [`Self::foreground_left`]。
    /// `None` 表示归属判不出来（当时前台是本进程），看门狗不生效。
    owner: Cell<Option<u32>>,
}

impl CandidateWindow {
    /// 建一个隐藏的候选窗口；点中候选或译词时调 `on_click`。
    pub(crate) fn new(painter: SharedPainter, on_click: CandidateClicks) -> Result<Self> {
        CLASS.ensure(|| WNDCLASSEXW {
            lpfnWndProc: Some(wndproc),
            hInstance: super::module_handle(),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        })?;
        let dpi = unsafe { GetDpiForSystem() }.max(96);
        let dark = resolve_dark(Appearance::default());
        let data = RefCell::new(RenderData::empty(Rc::new(Theme::new(dpi, dark))));
        // NOACTIVATE：显示时不抢应用焦点。鼠标只在候选与译词上收，其余穿透（见 [`click`]）。
        click::install(on_click);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                CLASS_NAME,
                w!("青简候选"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(super::module_handle()),
                None,
            )?
        };
        Ok(Self {
            hwnd,
            data,
            dpi: Cell::new(dpi),
            dark: Cell::new(dark),
            logged_dpi: Cell::new(None),
            painter,
            position: Cell::new((0, 0)),
            anchor: Cell::new(None),
            owner: Cell::new(None),
        })
    }

    /// 刷新内容（不定位、不显示）。
    pub(crate) fn set_content(&self, frame: &Frame, mode: Mode) {
        self.data.borrow_mut().set(frame, mode);
    }

    /// 设置改了（主题 / 配色 / 字号）：还显示着的候选窗按新设置就地重画一遍。
    /// 不重画的话屏幕上那块位图还留着上一套颜色，在设置里勾了开关像是没生效。
    pub(crate) fn repaint(&self) {
        let Some(anchor) = self.anchor.get() else {
            return;
        };
        if unsafe { IsWindowVisible(self.hwnd) }.as_bool() {
            self.show(anchor);
        }
    }

    /// 按光标矩形定位并显示：贴光标下方（放不下放上方），四周留出阴影。
    pub(crate) fn show(&self, anchor: RECT) {
        let was_visible = unsafe { IsWindowVisible(self.hwnd) }.as_bool();
        self.anchor.set(Some(anchor));
        self.sync_theme(anchor);
        let rendered = {
            let data = self.data.borrow();
            self.painter.borrow_mut().as_mut().and_then(|painter| {
                painter.render_frame(
                    &data.render_frame(),
                    data.layout,
                    self.dark.get(),
                    self.dpi.get(),
                )
            })
        };
        let updated = match rendered {
            Some(rendered) => {
                let content = (
                    rendered.content_width as i32,
                    rendered.content_height as i32,
                );
                if content.0 <= 0 || content.1 <= 0 {
                    self.hide();
                    return;
                }
                let overhang = (
                    rendered.content_y as i32,
                    rendered.pixmap.height() as i32 - rendered.content_y as i32 - content.1,
                );
                let (content_x, content_y) = place(anchor, content, overhang);
                let position = (
                    content_x - rendered.content_x as i32,
                    content_y - rendered.content_y as i32,
                );
                self.position.set(position);
                self.schedule(rendered.next_frame);
                click::set_hits(rendered.hits.clone(), (content_x, content_y));
                layered::present(self.hwnd, &rendered.pixmap, position)
            }
            None => {
                click::set_hits(Vec::new(), (0, 0));
                self.show_gdi(anchor)
            }
        };
        if updated.is_ok() {
            let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNA) };
            // 刚显示出来：记下前台是哪个线程（正在打字的应用），并起看门狗。
            // 已经显示着的那次（设置改了就地重画）不动记录，免得把归属记成本进程。
            if !was_visible {
                self.owner.set(foreground_thread().filter(|thread| {
                    // 前台是本进程（设置程序 / 状态条）时判不出归属，宁可不设看门狗
                    *thread != unsafe { GetCurrentThreadId() }
                }));
            }
            self.watch();
        } else {
            self.hide();
        }
    }

    /// GDI 画法：量尺寸、定位、合成。
    fn show_gdi(&self, anchor: RECT) -> Result<()> {
        let margin = layered::shadow_margin(self.dpi.get());
        let content = self.preferred_size();
        if content.0 <= 0 || content.1 <= 0 {
            return Err(Error::from(E_INVALIDARG));
        }
        // GDI 画法的投影顶影为 0，只有底边那一圈要避
        let (content_x, content_y) = place(anchor, content, (0, margin));
        let data = self.data.borrow();
        layered::composite(
            self.hwnd,
            &Layered {
                content,
                margin,
                win_pos: (content_x - margin, content_y - margin),
                win_size: (content.0 + margin * 2, content.1 + margin * 2),
                background: data.theme.background,
                corner_radius: data.theme.corner_radius,
                paint: &|hdc, client| view::paint(hdc, &data, client),
            },
        )
    }

    pub(crate) fn hide(&self) {
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        click::set_hits(Vec::new(), (0, 0));
        self.stop_animation();
        self.stop_watch();
        if let Some(painter) = self.painter.borrow_mut().as_mut() {
            painter.forget();
        }
    }

    /// 这条消息是候选窗的动画定时器。
    pub(crate) fn is_animation_timer(&self, msg: &MSG) -> bool {
        msg.message == WM_TIMER && msg.hwnd == self.hwnd && msg.wParam.0 == ANIMATION_TIMER
    }

    /// 这条消息是候选窗的前台看门狗。
    pub(crate) fn is_watchdog_timer(&self, msg: &MSG) -> bool {
        msg.message == WM_TIMER && msg.hwnd == self.hwnd && msg.wParam.0 == WATCHDOG_TIMER
    }

    /// 前台已经换到别的线程（切到别的应用 / 弹出别的窗口）了，候选窗该收。
    ///
    /// 为什么不能只靠 DLL：切到一个**没有输入框**的应用时，那条「本线程失去焦点」的通知未必会来
    /// （被切走的那个线程有没有焦点事件，取决于 TSF 怎么派），DLL 不说话，窗口就一直置顶盖着 ——
    /// 用户只能看着它，拖不动也关不掉。这条兜底由 Server 自己盯前台窗口。
    pub(crate) fn foreground_left(&self) -> bool {
        let Some(owner) = self.owner.get() else {
            // 归属没记下（显示时前台就是本进程）：看门狗不生效，交回 DLL 那套
            return false;
        };
        match foreground_thread() {
            Some(thread) => thread != owner,
            // 前台窗口拿不到（切桌面、安全桌面）：当作已经离开
            None => true,
        }
    }

    /// 起 / 续前台看门狗。同一个 id 再 `SetTimer` 只会改间隔，已开着时是无害的空操作。
    fn watch(&self) {
        unsafe { SetTimer(Some(self.hwnd), WATCHDOG_TIMER, WATCHDOG_INTERVAL, None) };
    }

    fn stop_watch(&self) {
        // SAFETY: 没在跳时 KillTimer 返回错误，忽略
        let _ = unsafe { KillTimer(Some(self.hwnd), WATCHDOG_TIMER) };
        self.owner.set(None);
    }

    /// 定时器每跳：要动画的下一帧贴在原处；播完就停。
    pub(crate) fn animation_frame(&self) {
        let rendered = self
            .painter
            .borrow_mut()
            .as_mut()
            .and_then(|painter| painter.tick());
        let Some(rendered) = rendered else {
            self.stop_animation();
            return;
        };
        let _ = layered::present(self.hwnd, &rendered.pixmap, self.position.get());
        self.schedule(rendered.next_frame);
    }

    /// 按渲染器给的间隔（过渡 16 ms、循环动画 33 ms）起或改定时器；`None` 停。同一个 id 再 SetTimer 就是改间隔。
    fn schedule(&self, next_frame: Option<std::time::Duration>) {
        match next_frame {
            Some(interval) => {
                let ms = interval.as_millis().clamp(10, 1000) as u32;
                // SAFETY: 本线程建的窗口；定时器消息由 UI 线程的消息循环截下交给 animation_frame
                unsafe { SetTimer(Some(self.hwnd), ANIMATION_TIMER, ms, None) };
            }
            None => self.stop_animation(),
        }
    }

    fn stop_animation(&self) {
        // SAFETY: 没在跳时 KillTimer 返回错误，忽略
        let _ = unsafe { KillTimer(Some(self.hwnd), ANIMATION_TIMER) };
    }

    /// DPI 或深浅变了就重建主题；每次 `show` 前调。
    ///
    /// DPI 取光标所在显示器的：窗口藏着时改了缩放（或睡眠唤醒后多显示器重排），
    /// `GetDpiForWindow` 会停在旧值，候选字就大小不对（#146）。
    fn sync_theme(&self, anchor: RECT) {
        let caret = POINT {
            x: anchor.left,
            y: anchor.top,
        };
        let monitor_dpi = monitor::dpi_near(caret);
        let window_dpi = unsafe { GetDpiForWindow(self.hwnd) };
        let dpi = match (monitor_dpi, window_dpi) {
            (Some(dpi), _) => dpi,
            (None, 0) => self.dpi.get(),
            (None, dpi) => dpi,
        };
        self.log_dpi(caret, window_dpi, monitor_dpi, dpi);
        let dark = resolve_dark(self.data.borrow().appearance);
        if dpi != self.dpi.get() || dark != self.dark.get() {
            self.data.borrow_mut().theme = Rc::new(Theme::new(dpi, dark));
            self.dpi.set(dpi);
            self.dark.set(dark);
        }
    }

    /// 缩放值变了就记一条，多显示器 / 睡眠唤醒的问题从日志里能看出取到的是哪个值（#146）。
    fn log_dpi(&self, caret: POINT, window_dpi: u32, monitor_dpi: Option<u32>, used: u32) {
        if self.logged_dpi.replace(Some((window_dpi, monitor_dpi)))
            == Some((window_dpi, monitor_dpi))
        {
            return;
        }
        tracing::info!(
            window_dpi,
            ?monitor_dpi,
            used,
            system_dpi = unsafe { GetDpiForSystem() },
            caret_x = caret.x,
            caret_y = caret.y,
            "候选窗口缩放值"
        );
    }

    /// 内容需要的大小（不含阴影留白）。
    fn preferred_size(&self) -> (i32, i32) {
        let hdc = unsafe { GetDC(Some(self.hwnd)) };
        let size = view::preferred_size(hdc, &self.data.borrow());
        unsafe { ReleaseDC(Some(self.hwnd), hdc) };
        (size.cx, size.cy)
    }
}

impl Drop for CandidateWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

/// 内容左上角：贴光标下方，放不下放上方，再放不下贴屏幕内；都夹在所在显示器工作区里。
/// `overhang` 是位图在内容区上、下伸出去的像素（投影、装饰），按主题而定：
/// 贴下方让开上沿那截，贴上方让开下沿那截，投影才不会罩在输入行刚打的字上（微信聊天框实测）。
fn place(anchor: RECT, content: (i32, i32), overhang: (i32, i32)) -> (i32, i32) {
    let work = monitor::work_area_near(POINT {
        x: anchor.left,
        y: anchor.top,
    });
    let x = anchor
        .left
        .clamp(work.left, (work.right - content.0).max(work.left));
    let below = anchor.bottom + CARET_GAP + overhang.0.max(0);
    let above = anchor.top - CARET_GAP - overhang.1.max(0) - content.1;
    let y = if below + content.1 <= work.bottom {
        below
    } else if above >= work.top {
        above
    } else {
        (work.bottom - content.1).max(work.top)
    };
    (x, y)
}

/// 分层窗口无需 `WM_PAINT`；鼠标交给 [`click`]，其余全交默认处理。
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => click::hit_test(lparam),
        WM_MOUSEACTIVATE => click::mouse_activate(),
        WM_LBUTTONDOWN => {
            click::button_down();
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
