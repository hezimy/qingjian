//! 组句收尾：DLL 说「收窗」时连缓冲一起清；连接断了（宿主进程退出）也要收窗。
//!
//! 这两个都对应「候选窗盖在屏幕上不走、点它上不了屏」那个毛病：DLL 在文档失活后写不了字，
//! 但它自己已经把组句判结束了，只能靠 Server 这边把窗口收掉。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use qingjian_core::Engine;
use qingjian_dictionary::Dictionary;
use qingjian_platform::protocol::{
    ClientMessage, Frame, KeyEvent, KeyModifiers, PROTOCOL_VERSION, ScreenRect, SessionId,
};
use qingjian_render::Mode;

use crate::dispatch::{CandidateSink, RenderSettings, Router, RouterConfig};

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
fn hiding_candidates_clears_the_buffer_too() {
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
        router.engine.composition().is_empty(),
        "收窗要连缓冲一起清：只为收窗的话，这个会话不再发键时拼音就永远留着"
    );
    assert!(hidden_count(&sink) > 0, "候选窗要被收起");
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
