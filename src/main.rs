#![windows_subsystem = "windows"]

mod anim;
mod app;
mod audio;
mod focus;
mod gfx;
mod i18n;
mod media;
mod privacy;
mod settings;
mod sys;
mod tray;
mod ui;

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DispatchMessageW, GetMessageW, PeekMessageW, PostQuitMessage, TranslateMessage,
    MSG, PM_REMOVE, WM_CONTEXTMENU, WM_DESTROY, WM_QUIT, WM_RBUTTONUP,
};

thread_local! {
    static APP: RefCell<Option<app::App>> = const { RefCell::new(None) };
}

/// Runs `f` on the app unless it is already borrowed further up the stack
/// (re-entrant messages sent by Win32 while we are inside a handler).
fn with_app<R>(f: impl FnOnce(&mut app::App) -> R) -> Option<R> {
    APP.with(|cell| {
        cell.try_borrow_mut()
            .ok()
            .and_then(|mut app| app.as_mut().map(f))
    })
}

fn main() {
    let Some(_instance) = sys::single_instance() else {
        return;
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let started = app::create_window(Some(wndproc)).and_then(app::App::new);
    match started {
        Ok(app) => APP.with(|cell| *cell.borrow_mut() = Some(app)),
        Err(e) => {
            sys::fatal(&format!("Isla could not start: {e}"));
            return;
        }
    }
    run_loop();
    APP.with(|cell| {
        if let Some(mut app) = cell.borrow_mut().take() {
            app.shutdown();
        }
    });
}

/// Sleeps in `GetMessageW` while idle. Only during an animation does it spin,
/// and then each frame waits for the monitor's vertical blank in `Present`.
fn run_loop() {
    let mut msg = MSG::default();
    loop {
        if with_app(|app| app.wants_frames()).unwrap_or(false) {
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            with_app(|app| app.frame());
        } else {
            unsafe {
                if !GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            with_app(|app| app.render_if_dirty());
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        app::WM_APP_TRAY => {
            let event = lp.0 as u32 & 0xFFFF;
            if event == WM_CONTEXTMENU || event == WM_RBUTTONUP {
                // The menu runs a modal loop that dispatches messages back here,
                // so it must run without holding the app borrowed.
                if let Some(state) = with_app(|app| app.menu_state()) {
                    let cmd = tray::show_menu(hwnd, &state);
                    with_app(|app| app.on_menu(cmd));
                }
                return LRESULT(0);
            }
        }
        _ => {}
    }
    if let Some(Some(result)) = with_app(|app| app.handle(msg, wp, lp)) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}
