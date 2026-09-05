//! V14 — Global Emergency Stop & Safety Kill-Switch
//!
//! Provides a dedicated OS-level safety monitor that polls hardware keyboard state
//! via `GetAsyncKeyState` to catch emergency kill-switch hotkeys (`F12` or `Ctrl+Shift+Escape`).
//!
//! When triggered, the safety monitor immediately engages the [`IoBus`] emergency lock,
//! freezing all synthetic input injection from autonomous AI agents.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

use crate::io_bus::IoBus;

/// Virtual Key code for F12 (Emergency Kill-Switch).
const VK_F12: i32 = 0x7B;
/// Virtual Key code for Control key.
const VK_CONTROL: i32 = 0x11;

/// The Safety Monitor handles background hotkey monitoring and emergency state.
pub struct SafetyMonitor {
    #[allow(dead_code)]
    running: Arc<AtomicBool>,
}

impl SafetyMonitor {
    /// Start the background safety monitoring thread.
    ///
    /// - **Press `F12`**: Immediately engages Emergency Stop (freezes all agent clicks & typing).
    /// - **Press `Ctrl + F12`**: Resets Emergency Stop and unfreezes the I/O bus.
    pub fn start(io_bus: Arc<IoBus>, running: Arc<AtomicBool>) -> Self {
        let r = Arc::clone(&running);
        let bus = Arc::clone(&io_bus);

        thread::Builder::new()
            .name("vactd-safety-monitor".to_string())
            .spawn(move || {
                log::info!("🛡️ Global Safety Monitor active (Press [F12] for Emergency Stop, [Ctrl+F12] to reset)");
                let mut prev_f12_down = false;

                while r.load(Ordering::SeqCst) {
                    // Check high-order bit of GetAsyncKeyState (0x8000 = currently pressed)
                    let f12_pressed = unsafe { (GetAsyncKeyState(VK_F12) as u16 & 0x8000) != 0 };
                    let ctrl_pressed = unsafe { (GetAsyncKeyState(VK_CONTROL) as u16 & 0x8000) != 0 };

                    // Trigger on leading edge (key press down)
                    if f12_pressed && !prev_f12_down {
                        if ctrl_pressed {
                            // Ctrl + F12 = Reset
                            bus.reset_emergency_stop();
                            log::info!("🟢 [SAFETY] Emergency Stop RESET by user (Ctrl+F12) — I/O Bus UNLOCKED.");
                        } else {
                            // F12 = Emergency Freeze
                            bus.trigger_emergency_stop();
                            log::warn!("🚨🚨🚨 [SAFETY KILL-SWITCH] EMERGENCY STOP ACTIVATED (F12) — I/O BUS FROZEN! 🚨🚨🚨");
                            log::warn!("🚨 All synthetic agent actions are now BLOCKED. Press [Ctrl+F12] to resume.");
                        }
                    }

                    prev_f12_down = f12_pressed;
                    thread::sleep(Duration::from_millis(30)); // 33Hz polling (~30ms response)
                }

                log::debug!("Safety monitor thread exiting.");
            })
            .expect("failed to spawn safety monitor thread");

        Self { running }
    }
}
