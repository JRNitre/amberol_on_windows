// SPDX-FileCopyrightText: 2026 Amberol contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#[cfg(target_os = "windows")]
mod native {
    use crate::window::Window;
    use gtk::{glib, prelude::*};
    use std::{
        mem::{size_of, zeroed},
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::*,
        Graphics::Dwm::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Shell::*, WindowsAndMessaging::*},
    };

    #[link(name = "gtk-4")]
    extern "C" {
        fn gdk_win32_surface_get_handle(surface: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }

    pub fn handle(window: &impl IsA<gtk::Window>) -> Option<HWND> {
        let surface = window.as_ref().surface()?;
        let hwnd = unsafe { gdk_win32_surface_get_handle(surface.as_ptr().cast()) } as HWND;
        (hwnd != 0).then_some(hwnd)
    }

    pub fn backdrop(window: &impl IsA<gtk::Window>, enabled: bool) -> bool {
        let Some(hwnd) = handle(window) else {
            return false;
        };
        // Documented system Acrylic, available from Windows 11 build 22621.
        // No global window opacity: foreground controls remain fully opaque.
        let kind: i32 = if enabled { 3 } else { 1 };
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE as u32,
                (&kind as *const i32).cast(),
                size_of::<i32>() as u32,
            ) >= 0
        }
    }

    pub fn topmost(window: &impl IsA<gtk::Window>, enabled: bool) -> bool {
        let Some(hwnd) = handle(window) else {
            return false;
        };
        unsafe {
            SetWindowPos(
                hwnd,
                if enabled {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                },
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            ) != 0
        }
    }

    pub fn resize(window: &impl IsA<gtk::Window>, width: i32, height: i32) {
        let Some(hwnd) = handle(window) else {
            return;
        };
        let widget = window.as_ref();
        let scale = widget.scale_factor();
        unsafe {
            let mut rect: RECT = zeroed();
            if GetWindowRect(hwnd, &mut rect) == 0 {
                return;
            }
            // Preserve GTK's client-side shadow extents and the current DPI.
            let extra_width = (rect.right - rect.left - widget.width() * scale).max(0);
            let extra_height = (rect.bottom - rect.top - widget.height() * scale).max(0);
            SetWindowPos(
                hwnd,
                0,
                0,
                0,
                width * scale + extra_width,
                height * scale + extra_height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    struct Context {
        window: glib::WeakRef<Window>,
        icon: NOTIFYICONDATAW,
        taskbar_created: u32,
    }
    pub struct Tray {
        hwnd: HWND,
        context: *mut Context,
    }
    impl std::fmt::Debug for Tray {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("WindowsTray")
        }
    }

    const CALLBACK: u32 = WM_APP + 72;
    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        let ctx = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Context;
        if !ctx.is_null() {
            let ctx = &mut *ctx;
            if message == ctx.taskbar_created {
                Shell_NotifyIconW(NIM_ADD, &ctx.icon);
            }
            if message == CALLBACK {
                if let Some(window) = ctx.window.upgrade() {
                    match lparam as u32 {
                        WM_LBUTTONUP => window.present(),
                        WM_RBUTTONUP | WM_CONTEXTMENU => {
                            let menu = CreatePopupMenu();
                            for (id, label) in [
                                (1, "显示播放器"),
                                (2, "播放 / 暂停"),
                                (3, "上一首"),
                                (4, "下一首"),
                                (5, "退出"),
                            ] {
                                let text: Vec<u16> = label.encode_utf16().chain(Some(0)).collect();
                                AppendMenuW(menu, MF_STRING, id, text.as_ptr());
                            }
                            let mut point: POINT = zeroed();
                            GetCursorPos(&mut point);
                            SetForegroundWindow(hwnd);
                            let choice = TrackPopupMenu(
                                menu,
                                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                                point.x,
                                point.y,
                                0,
                                hwnd,
                                ptr::null(),
                            );
                            DestroyMenu(menu);
                            match choice {
                                1 => window.present(),
                                2 => {
                                    let _ = gtk::prelude::WidgetExt::activate_action(
                                        &window, "win.play", None,
                                    );
                                }
                                3 => {
                                    let _ = gtk::prelude::WidgetExt::activate_action(
                                        &window,
                                        "win.previous",
                                        None,
                                    );
                                }
                                4 => {
                                    let _ = gtk::prelude::WidgetExt::activate_action(
                                        &window, "win.next", None,
                                    );
                                }
                                5 => {
                                    if let Some(app) = window.application() {
                                        app.activate_action("quit", None);
                                    }
                                }
                                _ => (),
                            }
                            PostMessageW(hwnd, WM_NULL, 0, 0);
                        }
                        _ => (),
                    }
                }
                return 0;
            }
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }

    impl Tray {
        pub fn new(window: &Window) -> Option<Self> {
            unsafe {
                let class: Vec<u16> = "AmberolTrayWindow".encode_utf16().chain(Some(0)).collect();
                let instance = GetModuleHandleW(ptr::null());
                let mut wc: WNDCLASSW = zeroed();
                wc.lpfnWndProc = Some(procedure);
                wc.hInstance = instance;
                wc.lpszClassName = class.as_ptr();
                RegisterClassW(&wc);
                // A hidden top-level window receives TaskbarCreated broadcasts.
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
                    ptr::null(),
                );
                if hwnd == 0 {
                    return None;
                }
                let mut icon: NOTIFYICONDATAW = zeroed();
                icon.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
                icon.hWnd = hwnd;
                icon.uID = 1;
                icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
                icon.uCallbackMessage = CALLBACK;
                icon.hIcon = handle(window)
                    .map(|h| SendMessageW(h, WM_GETICON, ICON_SMALL as usize, 0))
                    .unwrap_or(0);
                if icon.hIcon == 0 {
                    icon.hIcon = LoadIconW(0, IDI_APPLICATION);
                }
                let tip: Vec<u16> = "Amberol · 点击打开，右键控制".encode_utf16().collect();
                icon.szTip[..tip.len()].copy_from_slice(&tip);
                let event: Vec<u16> = "TaskbarCreated".encode_utf16().chain(Some(0)).collect();
                let context = Box::into_raw(Box::new(Context {
                    window: window.downgrade(),
                    icon,
                    taskbar_created: RegisterWindowMessageW(event.as_ptr()),
                }));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, context as isize);
                if Shell_NotifyIconW(NIM_ADD, &(*context).icon) == 0 {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    DestroyWindow(hwnd);
                    drop(Box::from_raw(context));
                    return None;
                }
                Some(Self { hwnd, context })
            }
        }
    }
    impl Drop for Tray {
        fn drop(&mut self) {
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &(*self.context).icon);
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                DestroyWindow(self.hwnd);
                drop(Box::from_raw(self.context));
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use native::*;

#[cfg(not(target_os = "windows"))]
pub fn backdrop(_: &impl gtk::prelude::IsA<gtk::Window>, _: bool) -> bool {
    false
}
#[cfg(not(target_os = "windows"))]
pub fn topmost(_: &impl gtk::prelude::IsA<gtk::Window>, _: bool) -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn resize(_: &impl gtk::prelude::IsA<gtk::Window>, _: i32, _: i32) {}
