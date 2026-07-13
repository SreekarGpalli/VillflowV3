//! Text injection — CONTRACTS §5 / §10.

use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use vf_core::InjectionMethod;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};

/// Inject `text` into the currently focused field.
pub fn inject_text(
    text: &str,
    method: InjectionMethod,
    restore_clipboard: bool,
) -> anyhow::Result<()> {
    match method {
        InjectionMethod::ClipboardPaste => inject_clipboard_paste(text, restore_clipboard),
        InjectionMethod::SendInputTyping => inject_sendinput_typing(text),
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
