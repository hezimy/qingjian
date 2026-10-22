//! 组句收尾：DLL 说「收窗」时连缓冲一起清；连接断了（宿主进程退出）也要收窗。
//!
//! 这两个都对应「候选窗盖在屏幕上不走、点它上不了屏」那个毛病：DLL 在文档失活后写不了字，
//! 但它自己已经把组句判结束了，只能靠 Server 这边把窗口收掉。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use qingjian_core::Engine;
use qingjian_dictionary::Dictionary;
use qingjian_platform::PreeditMode;
use qingjian_platform::protocol::{
    ClientMessage, Frame, KeyEvent, KeyModifiers, PROTOCOL_VERSION, ScreenRect, ServerMessage,
    SessionId,
};
use qingjian_render::Mode;

use crate::dispatch::{CandidateSink, PanelFrame, RenderSettings, Router, RouterConfig};

/// 记下候选窗被显示 / 收起的次数。
#[derive(Default)]
struct RecordingSink {
    shown: AtomicUsize,
    hidden: AtomicUsize,
}

impl CandidateSink for Arc<RecordingSink> {
    fn show(&self, _frame: Frame, _mode: Mode, _rect: ScreenRect) {
        self.shown.fetch_add(1, Ordering::Relaxed);
    }

    fn hide(&self) {
        self.hidden.fetch_add(1, Ordering::Relaxed);
    }

    fn show_panel(&self, _frame: PanelFrame, _rect: Option<ScreenRect>) {}

    fn hide_panel(&self) {}

    fn configure(&self, _settings: RenderSettings) {}
}

fn recording_router(sink: &Arc<RecordingSink>) -> Router {
    let engine = Engine::new(Dictionary::parse("你\tni\t100\n").unwrap());
    let mut router = Router::new(engine, RouterConfig::default());
    router.set_candidate_sink(Box::new(sink.clone()));
    router.handle(ClientMessage::OpenSession {
        session: SessionId(1),
        app: None,
        protocol: PROTOCOL_VERSION,
    });
    router
}

/// 敲几个字母起一段组句。
fn compose(router: &mut Router, text: &str) {
    for character in text.chars() {
        router.handle(ClientMessage::Key {
            session: SessionId(1),
            event: KeyEvent::new(character as u32, Some(character), KeyModifiers::default()),
        });
    }
}

fn hidden_count(sink: &Arc<RecordingSink>) -> usize {
    sink.hidden.load(Ordering::Relaxed)
}

#[test]
fn hiding_candidates_keeps_the_buffer() {
    let sink = Arc::new(RecordingSink::default());
    let mut router = recording_router(&sink);
    compose(&mut router, "ni");
    assert!(
        !router.engine.composition().is_empty(),
        "组句后缓冲里该有拼音"
    );

    router.handle(ClientMessage::HideCandidates {
        session: SessionId(1),
    });

    assert!(
        !router.engine.composition().is_empty(),
        "收窗不能连缓冲一起清：DLL 每拍没候选就发一次收窗，清缓冲会把正打着的拼音抹掉"
    );
    assert!(hidden_count(&sink) > 0, "候选窗要被收起");
}

#[test]
fn a_hide_between_two_keys_keeps_the_syllable() {
    let sink = Arc::new(RecordingSink::default());
    let mut router = recording_router(&sink);
    compose(&mut router, "x");
    // DLL 的轮询：`x` 还没成音节、这一帧没有候选，就发一次收窗。
    router.handle(ClientMessage::HideCandidates {
        session: SessionId(1),
    });
    compose(&mut router, "i");

    assert_eq!(
        router.engine.composition().text(),
        "xi",
        "收窗后接着打的字母要接上：清缓冲的话第二个字母会另起一段、原样上屏"
    );
}

#[test]
fn a_dropped_connection_hides_the_window() {
    let sink = Arc::new(RecordingSink::default());
    let mut router = recording_router(&sink);
    compose(&mut router, "ni");
    let before = hidden_count(&sink);

    // 宿主进程退出 / 管道出错：DLL 已经没机会发 `CloseSession` 了。
    router.handle_connection_closed(SessionId(1));

    assert!(
        hidden_count(&sink) > before,
        "连接断了要收起候选窗，不然它一直盖着别的界面"
    );
    assert!(router.engine.composition().is_empty(), "组句也要清掉");
}

#[test]
fn a_dropped_connection_leaves_other_sessions_alone() {
    let sink = Arc::new(RecordingSink::default());
    let mut router = recording_router(&sink);
    compose(&mut router, "ni");
    let before = hidden_count(&sink);

    router.handle_connection_closed(SessionId(2));

    assert_eq!(hidden_count(&sink), before, "别的会话断了不该收这轮的窗");
    assert!(
        !router.engine.composition().is_empty(),
        "别的会话断了不该动这轮组句"
    );
}

/// 指定宿主应用的 Router，拼音显示方式强制设成「只在候选窗口」（用户配置里的值）。
fn window_only_router(sink: &Arc<RecordingSink>, app: &str) -> Router {
    let engine = Engine::new(Dictionary::parse("你\tni\t100\n").unwrap());
    let mut router = Router::new(engine, RouterConfig::default());
    router.config.preedit = PreeditMode::Window;
    router.set_candidate_sink(Box::new(sink.clone()));
    router.handle(ClientMessage::OpenSession {
        session: SessionId(1),
        app: Some(app.to_owned()),
        protocol: PROTOCOL_VERSION,
    });
    router
}

fn key_frame(router: &mut Router, character: char) -> Frame {
    let response = router.handle(ClientMessage::Key {
        session: SessionId(1),
        event: KeyEvent::new(character as u32, Some(character), KeyModifiers::default()),
    });
    match response {
        Some(ServerMessage::KeyResult { frame, .. }) => frame,
        other => panic!("按键应有 KeyResult，实际 {other:?}"),
    }
}

// WPS 实测（2026-10-22 日志 pid 23980）：窗口模式里整个会话的选区恒定报在组句起点那个字上
// （(934,361,954,381) 一字不变），光标换行下移到页尾也不动，候选窗就钉死在起点。给它真实
// 组句范围（行内拼音）它才逐键报对位置，所以 WPS 的「只在候选窗口」自动当「行内+候选窗」。
#[test]
fn wps_gets_inline_preedit_even_in_window_only_mode() {
    let sink = Arc::new(RecordingSink::default());
    let mut router = window_only_router(&sink, "wps.exe");
    assert_eq!(key_frame(&mut router, 'n').preedit_mode, PreeditMode::Both);
}

#[test]
fn other_apps_keep_the_window_only_setting() {
    let sink = Arc::new(RecordingSink::default());
    let mut router = window_only_router(&sink, "Code.exe");
    assert_eq!(
        key_frame(&mut router, 'n').preedit_mode,
        PreeditMode::Window
    );
}
