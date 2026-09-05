use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use vact_protocol::ipc::IpcMessage;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, PIPE_UNLIMITED_INSTANCES,
};

const PIPE_ACCESS_DUPLEX: u32 = 3;

pub struct IpcServer {
    pipe_name: String,
    tcp_addr: Option<String>,
    clients: Arc<Mutex<Vec<mpsc::SyncSender<IpcMessage>>>>,
    latest_snapshot: Arc<Mutex<Option<vact_protocol::SceneGraph>>>,
}

impl IpcServer {
    pub fn new() -> Self {
        Self::with_config(r"\\.\pipe\VACT", Some("127.0.0.1:4242"))
    }

    pub fn with_config(pipe_name: impl Into<String>, tcp_addr: Option<impl Into<String>>) -> Self {
        Self {
            pipe_name: pipe_name.into(),
            tcp_addr: tcp_addr.map(|a| a.into()),
            clients: Arc::new(Mutex::new(Vec::new())),
            latest_snapshot: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(&self, io_bus: std::sync::Arc<crate::io_bus::IoBus>) {
        let clients_tcp = Arc::clone(&self.clients);
        let latest_snapshot_tcp = Arc::clone(&self.latest_snapshot);
        let bus_tcp = Arc::clone(&io_bus);

        // 1. TCP loopback server (High-throughput, concurrent duplex)
        if let Some(addr) = self.tcp_addr.clone() {
            thread::spawn(move || {
                let listener = match std::net::TcpListener::bind(&addr) {
                    Ok(l) => {
                        log::info!("IPC Server listening on TCP {}", addr);
                        l
                    }
                    Err(e) => {
                        log::warn!("Could not bind TCP IPC on {}: {}", addr, e);
                        return;
                    }
                };

                for stream_res in listener.incoming() {
                    match stream_res {
                        Ok(stream) => {
                            let _ = stream.set_nodelay(true);
                            log::info!("Client connected to IPC via TCP");
                            let (tx, rx) = mpsc::sync_channel::<IpcMessage>(128);

                            let snapshot = latest_snapshot_tcp.lock().unwrap().clone();
                            if let Some(snap) = snapshot {
                                let _ = tx.send(IpcMessage::Snapshot(snap));
                            }

                            clients_tcp.lock().unwrap().push(tx.clone());

                            let client_snap = Arc::clone(&latest_snapshot_tcp);
                            let bus = Arc::clone(&bus_tcp);
                            let write_tx = tx.clone();

                            let read_stream = match stream.try_clone() {
                                Ok(s) => s,
                                Err(_) => continue,
                            };
                            let write_stream = stream;

                            thread::spawn(move || {
                                handle_client(
                                    read_stream,
                                    write_stream,
                                    rx,
                                    write_tx,
                                    client_snap,
                                    bus,
                                );
                            });
                        }
                        Err(e) => {
                            log::debug!("TCP accept error: {}", e);
                        }
                    }
                }
            });
        }

        let clients = Arc::clone(&self.clients);
        let latest_snapshot = Arc::clone(&self.latest_snapshot);
        let pipe_name_str = self.pipe_name.clone();

        // 2. Windows Named Pipe server
        thread::spawn(move || {
            log::info!("IPC Server listening on {}", pipe_name_str);
            loop {
                let pipe_name: Vec<u16> = std::ffi::OsStr::new(&pipe_name_str)
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();

                let handle = unsafe {
                    CreateNamedPipeW(
                        PCWSTR(pipe_name.as_ptr()),
                        windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(PIPE_ACCESS_DUPLEX),
                        windows::Win32::System::Pipes::NAMED_PIPE_MODE(PIPE_TYPE_BYTE.0 | PIPE_READMODE_BYTE.0 | PIPE_WAIT.0 | PIPE_REJECT_REMOTE_CLIENTS.0),
                        PIPE_UNLIMITED_INSTANCES,
                        65536,
                        65536,
                        0,
                        None,
                    )
                };

                if handle == INVALID_HANDLE_VALUE {
                    log::error!("Failed to create named pipe");
                    thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }

                let connected = unsafe {
                    let res = ConnectNamedPipe(handle, None);
                    res.is_ok() || GetLastError() == ERROR_PIPE_CONNECTED
                };

                if connected {
                    log::info!("Client connected to IPC pipe");
                    let (tx, rx) = mpsc::sync_channel::<IpcMessage>(128);

                    let snapshot = latest_snapshot.lock().unwrap().clone();
                    if let Some(snap) = snapshot {
                        let _ = tx.send(IpcMessage::Snapshot(snap));
                    }

                    clients.lock().unwrap().push(tx.clone());

                    let client_snap = Arc::clone(&latest_snapshot);
                    let bus = std::sync::Arc::clone(&io_bus);
                    let write_tx = tx.clone();

                    let file = unsafe { File::from_raw_handle(handle.0 as _) };
                    let read_file = match file.try_clone() {
                        Ok(f) => f,
                        Err(_) => continue,
                    };
                    let write_file = file;

                    thread::spawn(move || {
                        handle_client(
                            read_file,
                            write_file,
                            rx,
                            write_tx,
                            client_snap,
                            bus,
                        );
                    });
                } else {
                    unsafe {
                        let _ = DisconnectNamedPipe(handle);
                    }
                }
            }
        });
    }

    pub fn update_latest_snapshot(&self, snap: vact_protocol::SceneGraph) {
        *self.latest_snapshot.lock().unwrap() = Some(snap);
    }

    pub fn broadcast(&self, msg: IpcMessage) {
        if let IpcMessage::Snapshot(snap) = &msg {
            *self.latest_snapshot.lock().unwrap() = Some(snap.clone());
        }

        let mut clients = self.clients.lock().unwrap();
        clients.retain(|tx| {
            match tx.try_send(msg.clone()) {
                Ok(_) => true,
                Err(mpsc::TrySendError::Full(_)) => {
                    log::warn!("Client pipe buffer full, dropping client");
                    false
                }
                Err(mpsc::TrySendError::Disconnected(_)) => false,
            }
        });
    }
}

fn handle_client<R, W>(
    mut reader: R,
    mut writer: W,
    rx: mpsc::Receiver<IpcMessage>,
    write_tx: mpsc::SyncSender<IpcMessage>,
    client_snap: Arc<Mutex<Option<vact_protocol::SceneGraph>>>,
    bus: Arc<crate::io_bus::IoBus>,
) where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    // Spawn writer thread
    thread::spawn(move || {
        while let Ok(msg) = rx.recv() {
            if let Err(e) = send_msg(&mut writer, &msg) {
                log::debug!("Client write loop terminated: {}", e);
                break;
            }
        }
    });

    // Read loop in current thread
    loop {
        match recv_msg(&mut reader) {
            Ok(IpcMessage::Handshake { protocol, capabilities }) => {
                log::info!("Client handshake: {} (caps: {:?})", protocol, capabilities);
                let vp = client_snap
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|s| s.viewport.clone())
                    .unwrap_or(vact_protocol::Viewport {
                        width: 1920,
                        height: 1080,
                        scale_factor: 1.0,
                    });
                let ack = IpcMessage::HandshakeAck { viewport: vp };
                let _ = write_tx.try_send(ack);
            }
            Ok(IpcMessage::Action { action, target_id, text, delta_y, vk, label, action_result }) => {
                use crate::io_bus::AgentAction;
                let agent_action = match action.as_str() {
                    "CLICK" => Some(AgentAction::Click { target_id }),
                    "TYPE" => text.map(|t| AgentAction::Type { target_id, text: t }),
                    "SCROLL" => Some(AgentAction::Scroll { target_id, delta_y: delta_y.unwrap_or(1) }),
                    "KEY" => vk.map(|k| AgentAction::Key { target_id, vk: k }),
                    "FOCUS" => Some(AgentAction::Focus { target_id }),
                    "LEARN" => label.map(|lbl| AgentAction::Learn { target_id, label: lbl, action_result }),
                    other => {
                        log::warn!("Unknown action type: {}", other);
                        None
                    }
                };

                if let Some(act) = agent_action {
                    let result = bus.dispatch(act);
                    log::info!(
                        "V11 I/O Bus: {} node={} route={:?} ok={} {:?}",
                        result.action, result.target_id, result.route, result.ok, result.error
                    );
                    let reply = IpcMessage::ActionResult {
                        action: result.action,
                        target_id: result.target_id,
                        route: format!("{:?}", result.route),
                        ok: result.ok,
                        error: result.error,
                    };
                    let _ = write_tx.try_send(reply);
                }
            }
            Ok(_) => {}
            Err(e) => {
                log::debug!("Client read loop terminated: {}", e);
                break;
            }
        }
    }
}

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
    if len > 10 * 1024 * 1024 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Message too large"));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    let msg = serde_json::from_slice(&payload)?;
    Ok(msg)
}
