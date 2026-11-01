//! Close to tray (Windows): the window hides from the taskbar and an icon
//! (the logo, resource 1 from build_icon.rs) appears in the notification
//! area. Clicking it brings MusiKPL back; its menu also offers Quit.
//!
//! The window is minimized and turned into a tool window rather than hidden,
//! because a minimized window keeps getting frames: playback, Auto Mix and
//! Jams keep running while it sits in the tray.

// The tray exists on Windows only.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrayEvent {
    Show,
    Quit,
}

#[cfg(windows)]
pub(crate) use windows::{hide_main_window, remember_main_window, show_main_window, Tray};

#[cfg(windows)]
mod windows {
    use std::{
        ptr::{null, null_mut},
        sync::{
            atomic::{AtomicBool, AtomicIsize, Ordering},
            mpsc::{self, Receiver, Sender},
            Mutex,
        },
    };

    use eframe::egui;
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
                NOTIFYICONDATAW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
                DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
                GetSystemMetrics, GetWindowLongPtrW, LoadImageW, PostMessageW, PostQuitMessage,
                RegisterClassW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TrackPopupMenu,
                TranslateMessage, GWL_EXSTYLE, IMAGE_ICON, LR_DEFAULTCOLOR, MF_STRING, MSG,
                SM_CXSMICON, SM_CYSMICON, SW_HIDE, SW_RESTORE, SW_SHOWMINNOACTIVE, TPM_NONOTIFY,
                TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY,
                WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_EX_APPWINDOW,
                WS_EX_TOOLWINDOW,
            },
        },
    };

    use super::TrayEvent;

    const CALLBACK: u32 = WM_APP + 1;
    const MENU_SHOW: usize = 1;
    const MENU_QUIT: usize = 2;

    static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);
    static TRAY_WINDOW: AtomicIsize = AtomicIsize::new(0);
    static STOP: AtomicBool = AtomicBool::new(false);
    /// Where the tray window's procedure reports clicks.
    static EVENTS: Mutex<Option<(Sender<TrayEvent>, egui::Context)>> = Mutex::new(None);

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// Remember MusiKPL's window, to hide and restore it.
    pub(crate) fn remember_main_window(hwnd: HWND) {
        MAIN_WINDOW.store(hwnd, Ordering::Relaxed);
    }

    /// Off the taskbar: minimized, as a tool window (which has no button).
    pub(crate) fn hide_main_window() {
        let hwnd = MAIN_WINDOW.load(Ordering::Relaxed);
        if hwnd == 0 {
            return;
        }
        // SAFETY: plain calls on a window handle owned by this process.
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                (style | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize),
            );
            ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
        }
    }

    /// Back on the taskbar, restored and in front.
    pub(crate) fn show_main_window() {
        let hwnd = MAIN_WINDOW.load(Ordering::Relaxed);
        if hwnd == 0 {
            return;
        }
        // SAFETY: as above.
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !(WS_EX_TOOLWINDOW as isize));
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }

    /// The tray icon; it disappears when this is dropped.
    pub(crate) struct Tray {
        events: Receiver<TrayEvent>,
    }

    impl Tray {
        pub(crate) fn show(ctx: &egui::Context) -> Option<Tray> {
            let (sender, events) = mpsc::channel();
            *EVENTS.lock().ok()? = Some((sender, ctx.clone()));
            STOP.store(false, Ordering::Relaxed);
            std::thread::spawn(run_tray);
            Some(Tray { events })
        }

        pub(crate) fn poll(&self) -> Option<TrayEvent> {
            self.events.try_recv().ok()
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            STOP.store(true, Ordering::Relaxed);
            let hwnd = TRAY_WINDOW.load(Ordering::Relaxed);
            if hwnd != 0 {
                // SAFETY: posting to a window of this process.
                unsafe {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                }
            }
        }
    }

    fn send(event: TrayEvent) {
        if let Ok(events) = EVENTS.lock() {
            if let Some((sender, ctx)) = events.as_ref() {
                let _ = sender.send(event);
                ctx.request_repaint();
            }
        }
    }

    /// The tray icon's own thread: a never-shown window that receives the
    /// icon's clicks, and its message loop.
    fn run_tray() {
        // SAFETY: standard Win32 window and notification-icon calls; every
        // pointer passed points to a live, correctly sized value.
        unsafe {
            let instance = GetModuleHandleW(null());
            let class = wide("MusiKPLTray");
            let window_class = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&window_class); // Fails harmlessly when registered before.
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                instance,
                null(),
            );
            if hwnd == 0 {
                return;
            }
            TRAY_WINDOW.store(hwnd, Ordering::Relaxed);
            // Resource 1 is the logo embedded by build_icon.rs; Windows takes
            // a resource number as a small pointer value (MAKEINTRESOURCE).
            #[allow(clippy::manual_dangling_ptr)]
            let icon = LoadImageW(
                instance,
                1 as *const u16,
                IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON),
                GetSystemMetrics(SM_CYSMICON),
                LR_DEFAULTCOLOR,
            );
            let mut data: NOTIFYICONDATAW = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = hwnd;
            data.uID = 1;
            data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
            data.uCallbackMessage = CALLBACK;
            data.hIcon = icon;
            let tip = wide("MusiKPL");
            data.szTip[..tip.len()].copy_from_slice(&tip);
            Shell_NotifyIconW(NIM_ADD, &data);

            // Dropped before the window existed: leave right away.
            if STOP.load(Ordering::Relaxed) {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            let mut message: MSG = std::mem::zeroed();
            while GetMessageW(&mut message, 0, 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            Shell_NotifyIconW(NIM_DELETE, &data);
            if icon != 0 {
                DestroyIcon(icon);
            }
            TRAY_WINDOW.store(0, Ordering::Relaxed);
        }
    }

    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            CALLBACK => {
                match (lparam as u32) & 0xFFFF {
                    WM_LBUTTONUP | WM_LBUTTONDBLCLK => send(TrayEvent::Show),
                    WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                    _ => {}
                }
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    /// Right-click menu: Open MusiKPL, Quit.
    unsafe fn show_menu(hwnd: HWND) {
        let menu = CreatePopupMenu();
        let open = wide("Open MusiKPL");
        let quit = wide("Quit");
        AppendMenuW(menu, MF_STRING, MENU_SHOW, open.as_ptr());
        AppendMenuW(menu, MF_STRING, MENU_QUIT, quit.as_ptr());
        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        // Needed so the menu closes when clicking elsewhere.
        SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            null_mut(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        match chosen as usize {
            MENU_SHOW => send(TrayEvent::Show),
            MENU_QUIT => send(TrayEvent::Quit),
            _ => {}
        }
    }
}

/// Other systems have no tray here: closing closes.
#[cfg(not(windows))]
pub(crate) struct Tray;

#[cfg(not(windows))]
impl Tray {
    pub(crate) fn show(_ctx: &eframe::egui::Context) -> Option<Tray> {
        None
    }

    pub(crate) fn poll(&self) -> Option<TrayEvent> {
        None
    }
}

#[cfg(not(windows))]
pub(crate) fn hide_main_window() {}

#[cfg(not(windows))]
pub(crate) fn show_main_window() {}
