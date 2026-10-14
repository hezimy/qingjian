use std::sync::mpsc::Sender;

use qingjian_platform::protocol::{ClientMessage, ServerMessage, SessionId};
use qingjian_render::HitTarget;

use crate::dispatch::StatusEvent;

/// 工人线程（独占 [`crate::dispatch::Router`]）的一件活：DLL 的一条消息，或状态条 / 候选窗口上的一次点击。
pub enum Work {
    /// 某条连接收到的消息 + 回结果的通道（`None` 表示不用回话）。
    Client(ClientMessage, Sender<Option<ServerMessage>>),

    /// UI 线程发来的状态条操作。
    Status(StatusEvent),

    /// UI 线程发来的候选窗口点击。
    Click(HitTarget),

    /// 某条连接断了（宿主进程退出 / 管道出错）：DLL 已经没机会发 `CloseSession` 了。
    ConnectionClosed(SessionId),
}
