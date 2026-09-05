use serde::{Deserialize, Serialize};

use crate::{diff::DiffFrame, SceneGraph, Viewport};

/// Standard VACT message framing: 4-byte LE length prefix + JSON payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IpcMessage {
    /// Client -> Server: Initial handshake.
    Handshake {
        protocol: String,
        capabilities: Vec<String>,
    },
    /// Server -> Client: Handshake acknowledgement with current viewport.
    HandshakeAck {
        viewport: Viewport,
    },
    /// Server -> Client: Full scene graph snapshot (sent on connect, or when requested).
    Snapshot(SceneGraph),
    /// Server -> Client: Temporal delta diff.
    Diff(DiffFrame),
    /// Client -> Server: Agent action to execute (V11).
    Action {
        action: String,
        target_id: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        delta_y: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        vk: Option<u16>,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        action_result: Option<String>,
    },
    /// Server -> Client: Result of an action dispatch (V11).
    ActionResult {
        action: String,
        target_id: u32,
        route: String,
        ok: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

