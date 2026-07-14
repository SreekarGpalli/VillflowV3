//! Text injection — CONTRACTS §5 / §10.

use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use vf_core::InjectionMethod;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

/// How text was (or should be) delivered after [`inject_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectOutcome {
    /// Sent via clipboard paste or SendInput to an external app.
    External,
    /// Foreground is a VillFlow window — shell must deliver via frontend event.
    InApp,
}

/// Inject `text` into the currently focused field.
///
/// When the focused window belongs to this process (Scratchpad / Settings),
/// returns [`InjectOutcome::InApp`] without using SendInput — WebView2 does not
/// reliably accept synthetic Ctrl+V or unicode input.
pub fn inject_text(
    text: &str,
    method: InjectionMethod,
    restore_clipboard: bool,
) -> anyhow::Result<InjectOutcome> {
    // The user may still be holding Ctrl/Shift from the push-to-talk chord.
    settle_modifiers(Duration::from_millis(800));

    if foreground_is_self() {
        force_release_all_modifiers();
        log::info!("inject: VillFlow window focused — in-app delivery");
        return Ok(InjectOutcome::InApp);
    }

    let result = match method {
        InjectionMethod::ClipboardPaste => inject_clipboard_paste(text, restore_clipboard),
        InjectionMethod::SendInputTyping => inject_sendinput_typing(text),
    };
    force_release_all_modifiers();
    result.map(|_| InjectOutcome::External)
}

/// True when the foreground top-level window is owned by this process, or its
/// title looks like a VillFlow window (Scratchpad / main).
pub fn foreground_is_self() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 && pid == GetCurrentProcessId() {
            return true;
        }
        // Fallback: title match (defensive if PID plumbing ever differs).
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len > 0 {
            let title = String::from_utf16_lossy(&buf[..len as usize]);
            let t = title.to_ascii_lowercase();
            if t.contains("scratchpad") || t == "villflow" || t.starts_with("villflow ") {
                return true;
            }
        }
        false
    }
}

/// Public: clear sticky modifiers after an utterance ends (even if inject was skipped).
pub fn force_release_all_modifiers() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
    };
    const MODS: [VIRTUAL_KEY; 11] = [
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_LWIN,
        VK_RWIN,
    ];
    let ups: Vec<INPUT> = MODS.into_iter().map(|vk| key_input(vk, true)).collect();
    unsafe {
        let _ = SendInput(&ups, std::mem::size_of::<INPUT>() as i32);
    }
}

fn modifiers_physically_down() -> Vec<VIRTUAL_KEY> {
    const MODS: [VIRTUAL_KEY; 5] = [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN];
    MODS.iter()
        .copied()
        .filter(|vk| unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 })
        .collect()
}

fn settle_modifiers(max_wait: Duration) {
    let deadline = Instant::now() + max_wait;
    while Instant::now() < deadline {
        if modifiers_physically_down().is_empty() {
            return;
        }
        thread::sleep(Duration::from_millis(15));
    }
    let held = modifiers_physically_down();
    if !held.is_empty() {
        let ups: Vec<INPUT> = held.into_iter().map(|vk| key_input(vk, true)).collect();
        unsafe {
            let _ = SendInput(&ups, std::mem::size_of::<INPUT>() as i32);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn inject_clipboard_paste(text: &str, restore_clipboard: bool) -> anyhow::Result<()> {
    let mut clip = Clipboard::new()?;
    let previous = if restore_clipboard {
        clip.get_text().ok()
    } else {
        None
    };

    clip.set_text(text.to_string())?;
    // Brief settle so the clipboard server publishes the new value before Ctrl+V.
    thread::sleep(Duration::from_millis(20));
    send_ctrl_v()?;
    // Wait for the target app to consume the paste before restoring. Too short
    // and some apps (Electron, Office) still read the restored previous clip.
    thread::sleep(Duration::from_millis(120));

    if restore_clipboard {
        match previous {
            Some(p) => {
                let _ = clip.set_text(p);
            }
            None => {
                let _ = clip.clear();
            }
        }
    }
    Ok(())
}

fn inject_sendinput_typing(text: &str) -> anyhow::Result<()> {
    for ch in text.encode_utf16() {
        send_unicode(ch, false)?;
        send_unicode(ch, true)?;
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

fn send_ctrl_v() -> anyhow::Result<()> {
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(VK_V, false),
        key_input(VK_V, true),
        key_input(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        anyhow::bail!("SendInput Ctrl+V failed");
    }
    Ok(())
}

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_unicode(unit: u16, up: bool) -> anyhow::Result<()> {
    let inputs = [INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: if up {
                    KEYEVENTF_KEYUP | KEYEVENTF_UNICODE
                } else {
                    KEYEVENTF_UNICODE
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        anyhow::bail!("SendInput unicode failed");
    }
    Ok(())
}
