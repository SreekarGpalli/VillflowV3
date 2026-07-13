//! Focused-app context via UI Automation — CONTRACTS §5.
//!
//! Best-effort: any failure returns partial/`None` fields rather than hard errors.

use std::time::{Duration, Instant};

use arboard::Clipboard;
use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
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
    /// Foreground HWND at capture time (for auto-learn re-read of same window).
    pub hwnd: isize,
}

thread_local! {
    static COM_INIT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn ensure_com() {
    COM_INIT.with(|c| {
        if !c.get() {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            c.set(true);
        }
    });
}

/// Read focus context. Never panics; returns empty defaults on total failure.
pub fn read_focus_context() -> FocusContext {
    let mut ctx = FocusContext::default();

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return ctx;
        }
        ctx.hwnd = hwnd.0 as isize;
        ctx.window_title = window_title(hwnd);
        ctx.app_name = process_exe_name(hwnd).unwrap_or_default();

        ensure_com();

        if let Ok(automation) =
            CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        {
            if let Ok(el) = automation.GetFocusedElement() {
                if let Some(text) = element_text_near_caret(&automation, &el) {
                    ctx.field_context = truncate_tail_chars(&text, FIELD_CONTEXT_CAP);
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

/// Simulated Ctrl+C selection fallback with a narrowed clipboard race window.
///
/// Uses a unique sentinel + `GetClipboardSequenceNumber` polling (short 5ms
/// ticks) instead of a fixed 40ms sleep, and aborts if the foreground window
/// changes mid-copy so foreign clipboard writes are not treated as selection.
fn selection_via_clipboard() -> Option<String> {
    let target_hwnd = unsafe { GetForegroundWindow() };
    if target_hwnd.0.is_null() {
        return None;
    }

    let mut clip = Clipboard::new().ok()?;
    let previous = clip.get_text().ok();

    // Sentinel proves the next non-matching clipboard text replaced our marker
    // (typically via Ctrl+C), not a stale previous value.
    let marker = format!(
        "__villflow_sel_{}_{}__",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    clip.set_text(marker.clone()).ok()?;

    let seq_before = unsafe { GetClipboardSequenceNumber() };

    unsafe {
        send_ctrl_key(0x43); // 'C'
    }

    let selected = poll_selection_clipboard(&mut clip, &marker, seq_before, target_hwnd);

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

/// Poll for a clipboard update that looks like our Ctrl+C result.
///
/// Max wait is short (~50ms) with 5ms steps so the clipboard is not held open
/// for a long fixed sleep while other apps could race it.
fn poll_selection_clipboard(
    clip: &mut Clipboard,
    marker: &str,
    seq_before: u32,
    target_hwnd: HWND,
) -> Option<String> {
    const STEP: Duration = Duration::from_millis(5);
    const BUDGET: Duration = Duration::from_millis(50);
    let deadline = Instant::now() + BUDGET;
    let mut saw_seq_change = false;

    loop {
        // Focus left the original app — do not trust clipboard contents.
        let fg = unsafe { GetForegroundWindow() };
        if fg != target_hwnd {
            return None;
        }

        let seq = unsafe { GetClipboardSequenceNumber() };
        if seq != seq_before {
            saw_seq_change = true;
            if let Ok(text) = clip.get_text() {
                if !text.is_empty() && text != marker {
                    return Some(text);
                }
            }
        }

        if Instant::now() >= deadline {
            if !saw_seq_change {
                return None;
            }
            let fg = unsafe { GetForegroundWindow() };
            if fg != target_hwnd {
                return None;
            }
            return clip
                .get_text()
                .ok()
                .filter(|s| !s.is_empty() && s != marker);
        }
        std::thread::sleep(STEP);
    }
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

/// Prefer text near the caret / selection; fall back to full value (tail-capped later).
unsafe fn element_text_near_caret(
    automation: &IUIAutomation,
    el: &IUIAutomationElement,
) -> Option<String> {
    // Text pattern: selection range first (caret neighborhood), else full document.
    if let Ok(unk) = el.GetCurrentPattern(UIA_TextPatternId) {
        if let Ok(text_pat) = unk.cast::<IUIAutomationTextPattern>() {
            // Prefer selection / caret range if present.
            if let Ok(ranges) = text_pat.GetSelection() {
                if let Ok(len) = ranges.Length() {
                    if len > 0 {
                        if let Ok(r) = ranges.GetElement(0) {
                            // Expand context: get surrounding text via GetText with a large cap.
                            if let Ok(bstr) = r.GetText(FIELD_CONTEXT_CAP as i32) {
                                let s = bstr.to_string();
                                if !s.is_empty() {
                                    // Also pull full document and take the window ending at selection
                                    // when selection is short (caret-only).
                                    if s.chars().count() < 40 {
                                        if let Ok(doc) = text_pat.DocumentRange() {
                                            if let Ok(full) = doc.GetText(-1) {
                                                let full_s = full.to_string();
                                                if !full_s.is_empty() {
                                                    return Some(full_s);
                                                }
                                            }
                                        }
                                    }
                                    return Some(s);
                                }
                            }
                        }
                    }
                }
            }
            if let Ok(range) = text_pat.DocumentRange() {
                if let Ok(bstr) = range.GetText(-1) {
                    let s = bstr.to_string();
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
            }
        }
    }

    // Prefer Value pattern (common for inputs) — take tail as "near caret".
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

/// Keep the *tail* of long text so context is near the caret (end of field).
fn truncate_tail_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        s.chars().skip(count - max).collect()
    }
}

/// Re-read text for auto-learn. Prefer the same HWND as injection time when possible.
#[allow(dead_code)]
pub fn reread_focused_text() -> Option<String> {
    reread_text_for_hwnd(None)
}

/// Re-read field text, optionally requiring the same window HWND as at inject time.
pub fn reread_text_for_hwnd(target_hwnd: Option<isize>) -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        if let Some(want) = target_hwnd {
            if want != 0 && (hwnd.0 as isize) != want {
                // Focus moved — silent no-op per §15.
                return None;
            }
        }
    }
    let ctx = read_focus_context();
    if ctx.field_context.is_empty() {
        None
    } else {
        Some(ctx.field_context)
    }
}
