//! Win32 layered Flow Bar overlay — CONTRACTS §5.
//!
//! `WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`, pill bottom-center,
//! never takes focus. Hidden when Idle.
//!
//! State is held in a replaceable `Mutex<Option<Arc<…>>>` (not `OnceLock`) so the
//! overlay can be shut down and started again during tests / engine respawn.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetDC, GetStockObject,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW, UpdateWindow, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, FW_SEMIBOLD, HBRUSH, OUT_DEFAULT_PRECIS,
    TRANSPARENT, WHITE_BRUSH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetSystemMetrics,
    LoadCursorW, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes,
    SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HICON,
    HWND_TOPMOST, IDC_ARROW, LWA_ALPHA, MSG, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WM_DESTROY, WM_PAINT, WM_USER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const WM_OVERLAY_CMD: u32 = WM_USER + 40;
const PILL_W: i32 = 200;
const PILL_H: i32 = 40;

#[derive(Debug, Clone)]
pub enum OverlayState {
    Hidden,
    Recording { level: f32 },
    Processing,
    Toast { message: String, until: Instant },
}

#[derive(Debug, Clone)]
pub enum OverlayCmd {
    Set(OverlayState),
    Shutdown,
}

/// HWND is not `Send` in windows-rs; only the overlay thread uses it.
struct SendHwnd(HWND);
// SAFETY: opaque OS handle stored for PostMessage from other threads.
unsafe impl Send for SendHwnd {}
unsafe impl Sync for SendHwnd {}

struct OverlayShared {
    state: Mutex<OverlayState>,
    hwnd: Mutex<Option<SendHwnd>>,
}

/// Process-global only because `wnd_proc` is a free Win32 callback. Replaced on
/// each `start()` and cleared on clean shutdown so the engine can respawn.
static SHARED: Mutex<Option<Arc<OverlayShared>>> = Mutex::new(None);
static STARTED: AtomicBool = AtomicBool::new(false);

fn current_shared() -> Option<Arc<OverlayShared>> {
    SHARED.lock().ok().and_then(|g| g.clone())
}

/// Start the overlay UI thread. Returns a command sender.
///
/// Safe to call again after a previous instance shut down (or while still
/// running — commands are forwarded to the live window).
pub fn start() -> mpsc::UnboundedSender<OverlayCmd> {
    let (tx, mut rx) = mpsc::unbounded_channel::<OverlayCmd>();

    if STARTED.swap(true, Ordering::SeqCst) {
        // Already running; forward cmds to the live window via SHARED.
        thread::spawn(move || {
            while let Some(cmd) = rx.blocking_recv() {
                apply_cmd(cmd);
            }
        });
        return tx;
    }

    // Fresh lifetime — install replaceable shared state (not OnceLock).
    let shared = Arc::new(OverlayShared {
        state: Mutex::new(OverlayState::Hidden),
        hwnd: Mutex::new(None),
    });
    if let Ok(mut slot) = SHARED.lock() {
        *slot = Some(shared);
    }

    thread::Builder::new()
        .name("vf-overlay".into())
        .spawn(move || {
            if let Err(e) = run_overlay_thread(&mut rx) {
                log::error!("overlay thread error: {e}");
            }
            STARTED.store(false, Ordering::SeqCst);
            if let Ok(mut slot) = SHARED.lock() {
                *slot = None;
            }
        })
        .expect("spawn overlay thread");

    tx
}

fn apply_cmd(cmd: OverlayCmd) {
    let Some(shared) = current_shared() else {
        return;
    };
    match cmd {
        OverlayCmd::Shutdown => {
            if let Some(SendHwnd(hwnd)) = *shared.hwnd.lock().unwrap() {
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_DESTROY, WPARAM(0), LPARAM(0));
                }
            }
        }
        OverlayCmd::Set(state) => {
            *shared.state.lock().unwrap() = state;
            if let Some(SendHwnd(hwnd)) = *shared.hwnd.lock().unwrap() {
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_OVERLAY_CMD, WPARAM(0), LPARAM(0));
                }
            }
        }
    }
}

fn run_overlay_thread(rx: &mut mpsc::UnboundedReceiver<OverlayCmd>) -> anyhow::Result<()> {
    unsafe {
        let class_name = wide("VillFlowOverlayClass");
        let h_instance = GetModuleHandleW(None)?;

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: h_instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            hIcon: HICON::default(),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(wide("VillFlow").as_ptr()),
            WS_POPUP,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            PILL_W,
            PILL_H,
            None,
            None,
            Some(h_instance.into()),
            None,
        )?;

        // Dark semi-opaque background.
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 220, LWA_ALPHA);

        if let Some(shared) = current_shared() {
            *shared.hwnd.lock().unwrap() = Some(SendHwnd(hwnd));
        }

        position_bottom_center(hwnd);
        let _ = ShowWindow(hwnd, SW_HIDE);

        // Pump: interleave GetMessage with channel polls via a timer-like peek loop.
        let mut msg = MSG::default();
        loop {
            // Drain overlay commands without blocking the message pump forever.
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    OverlayCmd::Shutdown => {
                        let _ = DestroyWindow(hwnd);
                        PostQuitMessage(0);
                    }
                    other => apply_cmd(other),
                }
            }

            // Peek/get with short timeout via PeekMessage + sleep.
            use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, PM_REMOVE};
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                // Expire toast.
                if let Some(shared) = current_shared() {
                    let mut st = shared.state.lock().unwrap();
                    if let OverlayState::Toast { until, .. } = &*st {
                        if Instant::now() >= *until {
                            *st = OverlayState::Hidden;
                            drop(st);
                            refresh_visibility(hwnd);
                        }
                    }
                }
                thread::sleep(Duration::from_millis(16));
            }
        }

        if let Some(shared) = current_shared() {
            *shared.hwnd.lock().unwrap() = None;
        }
    }
    Ok(())
}

unsafe fn position_bottom_center(hwnd: HWND) {
    let screen_w = GetSystemMetrics(SM_CXSCREEN);
    let screen_h = GetSystemMetrics(SM_CYSCREEN);
    let x = (screen_w - PILL_W) / 2;
    let y = screen_h - PILL_H - 48;
    let _ = MoveWindow(hwnd, x, y, PILL_W, PILL_H, true);
}

unsafe fn refresh_visibility(hwnd: HWND) {
    let Some(shared) = current_shared() else {
        return;
    };
    let state = shared.state.lock().unwrap().clone();
    match state {
        OverlayState::Hidden => {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        _ => {
            position_bottom_center(hwnd);
            // Re-assert TOPMOST on every show so the pill stays above Tauri
            // always-on-top windows (Scratchpad). Z-order among TOPMOST peers
            // is "last SetWindowPos wins".
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(hwnd);
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_OVERLAY_CMD => {
            refresh_visibility(hwnd);
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let (label, level) = {
        let state = current_shared()
            .map(|s| s.state.lock().unwrap().clone())
            .unwrap_or(OverlayState::Hidden);
        match state {
            OverlayState::Hidden => (String::new(), 0.0f32),
            OverlayState::Recording { level } => ("Recording".to_string(), level),
            OverlayState::Processing => ("Processing".to_string(), 0.0),
            OverlayState::Toast { message, .. } => (message, 0.0),
        }
    };

    // Background: RGB(0x1A, 0x1A, 0x22) dark blue-gray, stored as COLORREF 0x00BBGGRR.
    let brush = CreateSolidBrush(COLORREF(0x00221A1A));
    let rect = RECT {
        left: 0,
        top: 0,
        right: PILL_W,
        bottom: PILL_H,
    };
    let _ = FillRect(hdc, &rect, brush);
    let _ = DeleteObject(brush.into());

    // Level pulse bar under text when recording.
    // Pulse: RGB(0x60, 0xA0, 0xC8) soft blue, stored as COLORREF 0x00BBGGRR.
    if level > 0.01 {
        let bar_w = ((PILL_W as f32 - 24.0) * level.clamp(0.0, 1.0)) as i32;
        let pulse = CreateSolidBrush(COLORREF(0x00C8A060));
        let bar = RECT {
            left: 12,
            top: PILL_H - 8,
            right: 12 + bar_w.max(4),
            bottom: PILL_H - 4,
        };
        let _ = FillRect(hdc, &bar, pulse);
        let _ = DeleteObject(pulse.into());
    }

    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, COLORREF(0x00F0F0F0));

    let font = CreateFontW(
        18,
        0,
        0,
        0,
        FW_SEMIBOLD.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32,
        PCWSTR(wide("Segoe UI").as_ptr()),
    );
    let old = SelectObject(hdc, font.into());

    let text_w = wide(&label);
    let _ = TextOutW(hdc, 24, 10, &text_w[..text_w.len().saturating_sub(1)]);

    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(font.into());

    let _ = EndPaint(hwnd, &ps);
    let _ = (GetDC, ReleaseDC); // referenced for paint path completeness
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Helpers used by the orchestrator.
pub fn show_recording(tx: &mpsc::UnboundedSender<OverlayCmd>, level: f32) {
    let _ = tx.send(OverlayCmd::Set(OverlayState::Recording { level }));
}

pub fn show_processing(tx: &mpsc::UnboundedSender<OverlayCmd>) {
    let _ = tx.send(OverlayCmd::Set(OverlayState::Processing));
}

pub fn show_toast(tx: &mpsc::UnboundedSender<OverlayCmd>, message: impl Into<String>) {
    let _ = tx.send(OverlayCmd::Set(OverlayState::Toast {
        message: message.into(),
        until: Instant::now() + Duration::from_secs(2),
    }));
}

pub fn hide(tx: &mpsc::UnboundedSender<OverlayCmd>) {
    let _ = tx.send(OverlayCmd::Set(OverlayState::Hidden));
}
