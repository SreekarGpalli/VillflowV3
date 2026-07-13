//! Focused-app context via UI Automation — CONTRACTS §5.
//!
//! Best-effort: any failure returns partial/`None` fields rather than hard errors.

use std::time::Duration;

use arboard::Clipboard;
use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::ProcessStatus::K32GetModuleFileNameExW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationValuePattern, TreeScope_Children, UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

const FIELD_CONTEXT_CAP: usize = 1500;

/// Snapshot of the focused field / window at utterance start.
#[derive(Debug, Clone, Default)]
pub struct FocusContext {
    pub app_name: String,
    pub window_title: String,
    /// Text near the caret / field value, capped at ~1500 chars.
    pub field_context: String,
    /// Current selection if any (Command Mode).
    pub selection: Option<String>,
}

/// Read focus context. Never panics; returns empty defaults on total failure.
pub fn read_focus_context() -> FocusContext {
    let mut ctx = FocusContext::default();

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return ctx;
        }
        ctx.window_title = window_title(hwnd);
        ctx.app_name = process_exe_name(hwnd).unwrap_or_default();

        // UIA needs COM on this thread.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        if let Ok(automation) =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        {
            if let Ok(el) = automation.GetFocusedElement() {
                if let Some(text) = element_text(&automation, &el) {
                    ctx.field_context = truncate_chars(&text, FIELD_CONTEXT_CAP);
                }
                if let Some(sel) = element_selection(&el) {
                    if !sel.trim().is_empty() {
                        ctx.selection = Some(sel);
                    }
                }
            }
        }
    }

    ctx
}

/// Read selection: UIA first, then simulated Ctrl+C with full clipboard save/restore.
pub fn read_selection_with_fallback() -> Option<String> {
    let ctx = read_focus_context();
    if let Some(sel) = ctx.selection {
        if !sel.trim().is_empty() {
            return Some(sel);
        }
    }
    selection_via_clipboard()
}

fn selection_via_clipboard() -> Option<String> {
    let mut clip = Clipboard::new().ok()?;
    let previous = clip.get_text().ok();
    let _ = clip.set_text(String::new());

    unsafe {
        send_ctrl_key(0x43); // 'C'
    }
    std::thread::sleep(Duration::from_millis(40));

    let selected = clip.get_text().ok().filter(|s| !s.is_empty());

    match previous {
        Some(p) => {
            let _ = clip.set_text(p);
        }
        None => {
            let _ = clip.clear();
        }
    }
    selected
}

unsafe fn send_ctrl_key(vk: u16) {
    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    wScan: 0,
                    dwFlags: Default::default(),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: Default::default(),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];
    let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

unsafe fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, &mut buf);
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    }
}

unsafe fn process_exe_name(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return None;
    }
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buf = [0u16; MAX_PATH as usize];
    let len = K32GetModuleFileNameExW(Some(handle), None, &mut buf);
    let _ = CloseHandle(handle);
    if len == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    let name = path
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&path)
        .to_string();
    Some(name)
}

unsafe fn element_text(
    automation: &IUIAutomation,
    el: &IUIAutomationElement,
) -> Option<String> {
    // Prefer Value pattern.
    if let Ok(unk) = el.GetCurrentPattern(UIA_ValuePatternId) {
        if let Ok(pat) = unk.cast::<IUIAutomationValuePattern>() {
            if let Ok(bstr) = pat.CurrentValue() {
                let s = bstr.to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }

    // Text pattern document range.
    if let Ok(unk) = el.GetCurrentPattern(UIA_TextPatternId) {
        if let Ok(text_pat) = unk.cast::<IUIAutomationTextPattern>() {
            if let Ok(range) = text_pat.DocumentRange() {
                if let Ok(bstr) = range.GetText(FIELD_CONTEXT_CAP as i32) {
                    let s = bstr.to_string();
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
            }
        }
    }

    // Name fallback.
    if let Ok(bstr) = el.CurrentName() {
        let s = bstr.to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }

    // Shallow child walk for a value.
    if let Ok(cond) = automation.CreateTrueCondition() {
        if let Ok(children) = el.FindAll(TreeScope_Children, &cond) {
            let len = children.Length().unwrap_or(0).min(8);
            for i in 0..len {
                if let Ok(child) = children.GetElement(i) {
                    if let Ok(unk) = child.GetCurrentPattern(UIA_ValuePatternId) {
                        if let Ok(pat) = unk.cast::<IUIAutomationValuePattern>() {
                            if let Ok(bstr) = pat.CurrentValue() {
                                let s = bstr.to_string();
                                if !s.is_empty() {
                                    return Some(s);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

unsafe fn element_selection(el: &IUIAutomationElement) -> Option<String> {
    let unk = el.GetCurrentPattern(UIA_TextPatternId).ok()?;
    let text_pat = unk.cast::<IUIAutomationTextPattern>().ok()?;
    let ranges = text_pat.GetSelection().ok()?;
    let len = ranges.Length().ok()?;
    if len <= 0 {
        return None;
    }
    let mut out = String::new();
    for i in 0..len {
        if let Ok(r) = ranges.GetElement(i) {
            if let Ok(bstr) = r.GetText(-1) {
                out.push_str(&bstr.to_string());
            }
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}

/// Re-read the focused element's text (for auto-learn). Best-effort.
pub fn reread_focused_text() -> Option<String> {
    let ctx = read_focus_context();
    if ctx.field_context.is_empty() {
        None
    } else {
        Some(ctx.field_context)
    }
}
