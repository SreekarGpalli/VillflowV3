//! Global hotkeys via `WH_KEYBOARD_LL` — CONTRACTS §5.
//!
//! Push-to-talk needs key-up; events for matched combos are swallowed.
//!
//! Shared hook state is held in a replaceable `Mutex<Option<Arc<…>>>` (not
//! `OnceLock`) so a second engine spawn / test run can re-bind the event
//! channel and combos without being stuck on process-wide once-init state.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tokio::sync::mpsc;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Which logical hotkey fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyId {
    Dictation,
    CommandMode,
    Scratchpad,
}

/// Edge events from the hook thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// Combo fully matched (key-down of the final key while modifiers held).
    Down(HotkeyId),
    /// Combo released (any key of the combo went up while it was engaged).
    Up(HotkeyId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
    /// Virtual-key of the non-modifier key (uppercase letter or other VK).
    pub key_vk: u16,
}

impl KeyCombo {
    /// Parse strings like `Ctrl+Shift+Z`, `Ctrl+Shift+X`, case-insensitive.
    pub fn parse(s: &str) -> Option<Self> {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut win = false;
        let mut key_vk: Option<u16> = None;

        for part in s.split('+') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            let lower = p.to_ascii_lowercase();
            match lower.as_str() {
                "ctrl" | "control" => ctrl = true,
                "shift" => shift = true,
                "alt" | "menu" => alt = true,
                "win" | "super" | "meta" | "cmd" => win = true,
                _ => {
                    if p.chars().count() == 1 {
                        let c = p.chars().next()?.to_ascii_uppercase();
                        if c.is_ascii_alphanumeric() {
                            key_vk = Some(c as u16);
                        } else {
                            return None;
                        }
                    } else {
                        // Named keys we might need later.
                        let vk = match lower.as_str() {
                            "space" => Some(0x20u16),
                            "tab" => Some(0x09),
                            "enter" | "return" => Some(0x0D),
                            "esc" | "escape" => Some(0x1B),
                            _ => None,
                        }?;
                        key_vk = Some(vk);
                    }
                }
            }
        }
        Some(Self {
            ctrl,
            shift,
            alt,
            win,
            key_vk: key_vk?,
        })
    }

    pub fn matches_modifiers(&self, ctrl: bool, shift: bool, alt: bool, win: bool) -> bool {
        self.ctrl == ctrl && self.shift == shift && self.alt == alt && self.win == win
    }
}

#[derive(Clone)]
struct ComboSet {
    dictation: KeyCombo,
    command_mode: KeyCombo,
    scratchpad: KeyCombo,
}

struct HookShared {
    combos: Mutex<ComboSet>,
    /// Replaced on each `start()` so a respawned engine receives events.
    event_tx: Mutex<mpsc::UnboundedSender<HotkeyEvent>>,
    /// Currently engaged push-to-talk / hold combos (dictation & command).
    engaged: Mutex<HashSet<HotkeyId>>,
}

/// HWND/HHOOK raw pointers are not `Send` in windows-rs; the hook is only
/// touched from the hook thread + process teardown.
struct SendHook(HHOOK);
// SAFETY: HHOOK is an opaque OS handle; we only store/unhook it.
unsafe impl Send for SendHook {}
unsafe impl Sync for SendHook {}

/// Replaceable process-global state (required by the free-function hook proc).
static HOOK: Mutex<Option<SendHook>> = Mutex::new(None);
static SHARED: Mutex<Option<Arc<HookShared>>> = Mutex::new(None);
static HOOK_THREAD_RUNNING: AtomicBool = AtomicBool::new(false);

fn current_shared() -> Option<Arc<HookShared>> {
    SHARED.lock().ok().and_then(|g| g.clone())
}

/// Start the low-level keyboard hook on a dedicated thread.
///
/// Returns a channel of [`HotkeyEvent`]s. Call [`update_combos`] after settings changes.
/// Safe to call again after a prior engine lifetime: combos and the event sender
/// are rebound even if the OS hook thread is already running.
pub fn start(
    dictation: &str,
    command_mode: &str,
    scratchpad: &str,
) -> anyhow::Result<mpsc::UnboundedReceiver<HotkeyEvent>> {
    let (tx, rx) = mpsc::unbounded_channel();
    let combos = ComboSet {
        dictation: KeyCombo::parse(dictation)
            .ok_or_else(|| anyhow::anyhow!("invalid dictation hotkey: {dictation}"))?,
        command_mode: KeyCombo::parse(command_mode)
            .ok_or_else(|| anyhow::anyhow!("invalid command_mode hotkey: {command_mode}"))?,
        scratchpad: KeyCombo::parse(scratchpad)
            .ok_or_else(|| anyhow::anyhow!("invalid scratchpad hotkey: {scratchpad}"))?,
    };

    {
        let mut slot = SHARED
            .lock()
            .map_err(|_| anyhow::anyhow!("hotkey shared state poisoned"))?;
        if let Some(existing) = slot.as_ref() {
            // Rebind to the new engine instance without restarting the hook thread.
            *existing.combos.lock().unwrap() = combos;
            *existing.event_tx.lock().unwrap() = tx;
            existing.engaged.lock().unwrap().clear();
            return Ok(rx);
        }
        *slot = Some(Arc::new(HookShared {
            combos: Mutex::new(combos),
            event_tx: Mutex::new(tx),
            engaged: Mutex::new(HashSet::new()),
        }));
    }

    if HOOK_THREAD_RUNNING.swap(true, Ordering::SeqCst) {
        // Shared was empty but the flag said running (race after teardown) —
        // the new SHARED above is enough; the live hook will pick it up.
        return Ok(rx);
    }

    thread::Builder::new()
        .name("vf-hotkeys".into())
        .spawn(|| {
            unsafe {
                let hook = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(low_level_proc),
                    None,
                    0,
                );
                match hook {
                    Ok(h) => {
                        *HOOK.lock().unwrap() = Some(SendHook(h));
                        // Message pump required for LL hooks on this thread.
                        let mut msg = windows::Win32::UI::WindowsAndMessaging::MSG::default();
                        while windows::Win32::UI::WindowsAndMessaging::GetMessageW(
                            &mut msg, None, 0, 0,
                        )
                        .as_bool()
                        {
                            let _ = windows::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
                            windows::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
                        }
                        if let Some(SendHook(h)) = HOOK.lock().unwrap().take() {
                            let _ = UnhookWindowsHookEx(h);
                        }
                    }
                    Err(e) => {
                        log::error!("SetWindowsHookExW failed: {e}");
                    }
                }
            }
            HOOK_THREAD_RUNNING.store(false, Ordering::SeqCst);
            if let Ok(mut slot) = SHARED.lock() {
                *slot = None;
            }
        })?;

    // Brief settle so the hook is installed before first keypress.
    thread::sleep(Duration::from_millis(20));
    Ok(rx)
}

pub fn update_combos(dictation: &str, command_mode: &str, scratchpad: &str) -> anyhow::Result<()> {
    let Some(shared) = current_shared() else {
        return Ok(());
    };
    let set = ComboSet {
        dictation: KeyCombo::parse(dictation)
            .ok_or_else(|| anyhow::anyhow!("invalid dictation hotkey: {dictation}"))?,
        command_mode: KeyCombo::parse(command_mode)
            .ok_or_else(|| anyhow::anyhow!("invalid command_mode hotkey: {command_mode}"))?,
        scratchpad: KeyCombo::parse(scratchpad)
            .ok_or_else(|| anyhow::anyhow!("invalid scratchpad hotkey: {scratchpad}"))?,
    };
    *shared.combos.lock().unwrap() = set;
    Ok(())
}

fn modifiers_down() -> (bool, bool, bool, bool) {
    unsafe {
        let ctrl = key_down(VK_CONTROL.0) || key_down(VK_LCONTROL.0) || key_down(VK_RCONTROL.0);
        let shift = key_down(VK_SHIFT.0) || key_down(VK_LSHIFT.0) || key_down(VK_RSHIFT.0);
        let alt = key_down(VK_MENU.0) || key_down(VK_LMENU.0) || key_down(VK_RMENU.0);
        let win = key_down(VK_LWIN.0) || key_down(VK_RWIN.0);
        (ctrl, shift, alt, win)
    }
}

unsafe fn key_down(vk: u16) -> bool {
    GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0
}

fn is_ctrl_vk(vk: u16) -> bool {
    vk == VK_CONTROL.0 || vk == VK_LCONTROL.0 || vk == VK_RCONTROL.0
}
fn is_shift_vk(vk: u16) -> bool {
    vk == VK_SHIFT.0 || vk == VK_LSHIFT.0 || vk == VK_RSHIFT.0
}
fn is_alt_vk(vk: u16) -> bool {
    vk == VK_MENU.0 || vk == VK_LMENU.0 || vk == VK_RMENU.0
}
fn is_win_vk(vk: u16) -> bool {
    vk == VK_LWIN.0 || vk == VK_RWIN.0
}

fn is_modifier_vk(vk: u16) -> bool {
    is_ctrl_vk(vk) || is_shift_vk(vk) || is_alt_vk(vk) || is_win_vk(vk)
}

fn combo_involves(combo: &KeyCombo, vk: u16) -> bool {
    if combo.key_vk == vk {
        return true;
    }
    if combo.ctrl && is_ctrl_vk(vk) {
        return true;
    }
    if combo.shift && is_shift_vk(vk) {
        return true;
    }
    if combo.alt && is_alt_vk(vk) {
        return true;
    }
    if combo.win && is_win_vk(vk) {
        return true;
    }
    false
}

unsafe extern "system" fn low_level_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code as u32 != HC_ACTION {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let Some(shared) = current_shared() else {
        return CallNextHookEx(None, code, wparam, lparam);
    };

    let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    // Don't swallow our own injected keys (paste / typing).
    if kb.flags.contains(LLKHF_INJECTED) {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let vk = kb.vkCode as u16;
    let msg = wparam.0 as u32;
    let is_down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_up = matches!(msg, WM_KEYUP | WM_SYSKEYUP);

    if !is_down && !is_up {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let combos = shared.combos.lock().unwrap().clone();
    let (ctrl, shift, alt, win) = modifiers_down();

    // Adjust modifiers for the event currently in flight (GetAsyncKeyState may lag).
    let (ctrl, shift, alt, win) = adjust_mods_for_event(vk, is_down, ctrl, shift, alt, win);

    let mut swallow = false;
    let event_tx = shared.event_tx.lock().unwrap().clone();

    if is_down && !is_modifier_vk(vk) {
        // Match main key + modifiers.
        let candidates = [
            (HotkeyId::Dictation, &combos.dictation),
            (HotkeyId::CommandMode, &combos.command_mode),
            (HotkeyId::Scratchpad, &combos.scratchpad),
        ];
        for (id, combo) in candidates {
            if combo.key_vk == vk && combo.matches_modifiers(ctrl, shift, alt, win) {
                swallow = true;
                let mut engaged = shared.engaged.lock().unwrap();
                if !engaged.contains(&id) {
                    engaged.insert(id);
                    let _ = event_tx.send(HotkeyEvent::Down(id));
                }
            }
        }
    }

    if is_up {
        let mut engaged = shared.engaged.lock().unwrap();
        let ids: Vec<HotkeyId> = engaged.iter().copied().collect();
        for id in ids {
            let combo = match id {
                HotkeyId::Dictation => &combos.dictation,
                HotkeyId::CommandMode => &combos.command_mode,
                HotkeyId::Scratchpad => &combos.scratchpad,
            };
            if combo_involves(combo, vk) {
                swallow = true;
                engaged.remove(&id);
                let _ = event_tx.send(HotkeyEvent::Up(id));
            }
        }
        // Also swallow modifier/key ups that belong to a configured combo even if not engaged,
        // when the key is part of a combo that is currently fully held (edge cases).
        for combo in [&combos.dictation, &combos.command_mode, &combos.scratchpad] {
            if combo_involves(combo, vk) && combo.matches_modifiers(ctrl, shift, alt, win)
                || (combo.key_vk == vk)
            {
                // Swallow ups of the main key when modifiers still match the combo shape.
                if combo.key_vk == vk {
                    swallow = true;
                }
            }
        }
    }

    // While a combo is engaged, swallow all of its constituent keys.
    {
        let engaged = shared.engaged.lock().unwrap();
        for id in engaged.iter() {
            let combo = match id {
                HotkeyId::Dictation => &combos.dictation,
                HotkeyId::CommandMode => &combos.command_mode,
                HotkeyId::Scratchpad => &combos.scratchpad,
            };
            if combo_involves(combo, vk) {
                swallow = true;
            }
        }
    }

    if swallow {
        LRESULT(1) // non-zero = swallow
    } else {
        CallNextHookEx(None, code, wparam, lparam)
    }
}

/// Align tracked modifier flags with the event currently being processed.
///
/// `GetAsyncKeyState` can lag the LL hook by one event; we force the in-flight
/// key's state on both key-down **and** key-up so modifiers cannot stick.
fn adjust_mods_for_event(
    vk: u16,
    is_down: bool,
    mut ctrl: bool,
    mut shift: bool,
    mut alt: bool,
    mut win: bool,
) -> (bool, bool, bool, bool) {
    if is_ctrl_vk(vk) {
        ctrl = is_down;
    }
    if is_shift_vk(vk) {
        shift = is_down;
    }
    if is_alt_vk(vk) {
        alt = is_down;
    }
    if is_win_vk(vk) {
        win = is_down;
    }
    (ctrl, shift, alt, win)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ctrl_shift_z() {
        let c = KeyCombo::parse("Ctrl+Shift+Z").unwrap();
        assert!(c.ctrl && c.shift && !c.alt && !c.win);
        assert_eq!(c.key_vk, b'Z' as u16);
    }

    #[test]
    fn parse_case_insensitive() {
        let c = KeyCombo::parse("ctrl+shift+x").unwrap();
        assert_eq!(c.key_vk, b'X' as u16);
    }

    #[test]
    fn adjust_mods_clears_on_keyup() {
        // Ctrl down → true; Ctrl up → false (previously stuck true).
        let (c, s, a, w) = adjust_mods_for_event(VK_CONTROL.0, true, false, false, false, false);
        assert!(c && !s && !a && !w);
        let (c, s, a, w) = adjust_mods_for_event(VK_CONTROL.0, false, true, true, false, false);
        assert!(!c && s && !a && !w);

        let (c, s, a, w) = adjust_mods_for_event(VK_LSHIFT.0, true, false, false, false, false);
        assert!(!c && s && !a && !w);
        let (c, s, a, w) = adjust_mods_for_event(VK_LSHIFT.0, false, false, true, false, false);
        assert!(!c && !s && !a && !w);
    }
}
