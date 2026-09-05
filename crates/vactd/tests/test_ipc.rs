use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use vact_protocol::ipc::IpcMessage;
use vact_protocol::{SceneGraph, SceneNode, Viewport};
use vactd::io_bus::IoBus;
use vactd::ipc::IpcServer;

fn send_msg<W: Write>(writer: &mut W, msg: &IpcMessage) -> std::io::Result<()> {
    let json = serde_json::to_string(msg)?;
    let bytes = json.as_bytes();
    let len = bytes.len() as u32;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}

fn recv_msg<R: Read>(reader: &mut R) -> std::io::Result<IpcMessage> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    let msg = serde_json::from_slice(&payload)?;
    Ok(msg)
}

#[test]
fn test_ipc_server_client() {
    let test_port = "127.0.0.1:4299";
    let test_pipe = r"\\.\pipe\VACT_TEST_PIPE_UNIT";

    let ipc_server = IpcServer::with_config(test_pipe, Some(test_port));
    ipc_server.start(Arc::new(IoBus::new()));

    // Give server thread time to bind
    thread::sleep(Duration::from_millis(150));

    // Connect via TCP
    let mut client = TcpStream::connect(test_port).expect("Failed to connect to test TCP IPC server");
    client.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    client.set_write_timeout(Some(Duration::from_secs(3))).unwrap();

    // 1. Send Handshake
    let handshake = IpcMessage::Handshake {
        protocol: "VACT/1.0".to_string(),
        capabilities: vec!["test".to_string()],
    };
    send_msg(&mut client, &handshake).expect("Failed to send handshake");

    // 2. Receive HandshakeAck
    let ack = recv_msg(&mut client).expect("Failed to receive HandshakeAck");
    match ack {
        IpcMessage::HandshakeAck { viewport } => {
            assert_eq!(viewport.width, 1920);
            assert_eq!(viewport.height, 1080);
        }
        other => panic!("Expected HandshakeAck, got: {:?}", other),
    }

    // 3. Broadcast a snapshot from the server
    let dummy_snapshot = SceneGraph::new(
        1,
        123456,
        Viewport {
            width: 1920,
            height: 1080,
            scale_factor: 1.0,
        },
        Some("TestWindow".to_string()),
        SceneNode::container(1, [0, 0, 1920, 1080]),
    );

    ipc_server.broadcast(IpcMessage::Snapshot(dummy_snapshot));

    // 4. Read the snapshot on the client
    let snap_msg = recv_msg(&mut client).expect("Failed to read snapshot");
    match snap_msg {
        IpcMessage::Snapshot(snap) => {
            assert_eq!(snap.seq, 1);
            assert_eq!(snap.viewport.width, 1920);
            assert_eq!(snap.active_window.as_deref(), Some("TestWindow"));
        }
        other => panic!("Expected Snapshot message, got: {:?}", other),
    }

    // 5. Send an action and verify reply
    let action_msg = IpcMessage::Action {
        action: "CLICK".to_string(),
        target_id: 1,
        text: None,
        delta_y: None,
        vk: None,
        label: None,
        action_result: None,
    };
    send_msg(&mut client, &action_msg).expect("Failed to send Action");

    let reply = recv_msg(&mut client).expect("Failed to receive ActionResult");
    match reply {
        IpcMessage::ActionResult { action, target_id, .. } => {
            assert_eq!(action, "CLICK");
            assert_eq!(target_id, 1);
        }
        other => panic!("Expected ActionResult, got: {:?}", other),
    }
}
