//! Text injection — CONTRACTS §5 / §10.

use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use vf_core::InjectionMethod;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};

/// Inject `text` into the currently focused field.
pub fn inject_text(
    text: &str,
    method: InjectionMethod,
    restore_clipboard: bool,
) -> anyhow::Result<()> {
    // The user may still be holding Ctrl/Shift from the push-to-talk chord.
    // Injecting Ctrl+V (or typing) under held modifiers turns it into e.g.
    // Ctrl+Shift+V inside the target app — wait for release, then force-clear
    // anything still held.
    settle_modifiers(Duration::from_millis(800));
    let result = match method {
        InjectionMethod::ClipboardPaste => inject_clipboard_paste(text, restore_clipboard),
        InjectionMethod::SendInputTyping => inject_sendinput_typing(text),
    };
    // Leave the OS with a clean modifier state so mouse/keyboard feel normal
    // after dictation (no phantom Shift+click).
    force_release_all_modifiers();
    result
}

/// Public: clear sticky modifiers after an utterance ends (even if inject was skipped).
pub fn force_release_all_modifiers() {
    const MODS: [VIRTUAL_KEY; 5] = [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN];
    // Send key-ups for every standard modifier. Harmless if already up.
    let ups: Vec<INPUT> = MODS.into_iter().map(|vk| key_input(vk, true)).collect();
    unsafe {
        let _ = SendInput(&ups, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Physical modifiers currently reported down by the OS.
fn modifiers_physically_down() -> Vec<VIRTUAL_KEY> {
    const MODS: [VIRTUAL_KEY; 5] = [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN];
    MODS.iter()
        .copied()
        .filter(|vk| unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 })
        .collect()
}

/// Wait (bounded) for the push-to-talk modifiers to be physically released;
/// if any are still held at the deadline, synthesize key-ups so the injected
/// input sequence starts from a clean modifier state. A later physical release
/// just produces a redundant key-up, which is harmless.
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
        unsafe { SendInput(&ups, std::mem::size_of::<INPUT>() as i32) };
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
    // Small delay so the target app sees the new clipboard.
    thread::sleep(Duration::from_millis(15));
    send_ctrl_v()?;
    // Settle before restore.
    thread::sleep(Duration::from_millis(80));

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
        // Tiny pacing so some hosts do not drop unicode input bursts.
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
