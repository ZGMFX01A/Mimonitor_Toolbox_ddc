pub mod catalog;
use parking_lot::Mutex;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{RedrawWindow, RDW_FRAME, RDW_INVALIDATE, RDW_UPDATENOW};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, EnumThreadWindows, GetClassNameW, GetCursorPos, GetMessageW, IsWindowVisible,
    KillTimer, LoadIconW, PostMessageW, PostQuitMessage, SetTimer,
    RegisterClassW, SetForegroundWindow, TrackPopupMenuEx, TranslateMessage, CW_USEDEFAULT,
    MF_SEPARATOR, MF_STRING, TPM_NOANIMATION, TPM_RETURNCMD, WM_CLOSE, WM_DESTROY,
    WM_INITMENUPOPUP, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_TIMER, WM_USER, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

/// 托盘后台线程向 GPUI 线程桥接分发的强类型事件
#[derive(Debug, Clone)]
pub enum TrayEvent {
    OpenMainWindow,
    ToggleBrightnessPanel,
    RefreshMonitors,
    Quit,
    Custom(catalog::Choice),
    OpenValueSlider(crate::hotkey::AdjustParam),
}

const WM_TRAY_CALLBACK: u32 = WM_USER + 101;
const CMD_OPEN_MAIN: usize = 1001;
const CMD_TOGGLE_BRIGHTNESS: usize = 1002;
const CMD_REFRESH_MONITORS: usize = 1003;
const CMD_QUIT: usize = 1004;

static MENU_CHOICES: Mutex<Vec<catalog::Choice>> = Mutex::new(Vec::new());
static EVENT_SENDER: Mutex<Option<Sender<TrayEvent>>> = Mutex::new(None);

thread_local! {
    static MENU_PAINT_TIMER: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static MENU_PAINT_SERIAL: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

unsafe fn cancel_menu_paint(hwnd: HWND) {
    let timer = MENU_PAINT_TIMER.replace(0);
    if timer != 0 {
        let _ = KillTimer(Some(hwnd), timer);
    }
}

unsafe extern "system" fn paint_visible_menu(hwnd: HWND, _: LPARAM) -> BOOL {
    let mut class_name = [0u16; 32];
    let length = GetClassNameW(hwnd, &mut class_name) as usize;
    if IsWindowVisible(hwnd).as_bool() && &class_name[..length] == w!("#32768").as_wide() {
        let _ = RedrawWindow(
            Some(hwnd), None, None, RDW_INVALIDATE | RDW_UPDATENOW | RDW_FRAME,
        );
    }
    BOOL(1)
}

/// 系统托盘管理器
pub struct SystemTrayManager {
    event_receiver: Arc<Mutex<Receiver<TrayEvent>>>,
    hwnd_raw: Arc<Mutex<Option<isize>>>,
    worker_thread: Arc<Mutex<Option<thread::JoinHandle<()>>>>,
}

impl Default for SystemTrayManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemTrayManager {
    pub fn new() -> Self {
        let (tx, rx) = channel::<TrayEvent>();
        *EVENT_SENDER.lock() = Some(tx);
        Self {
            event_receiver: Arc::new(Mutex::new(rx)),
            hwnd_raw: Arc::new(Mutex::new(None)),
            worker_thread: Arc::new(Mutex::new(None)),
        }
    }

    /// 启动 Win32 托盘图标及后台专属消息循环（安全桥接 GPUI 线程）
    pub fn start(&self) {
        let hwnd_holder = self.hwnd_raw.clone();
        let handle = thread::spawn(move || unsafe {
            let class_name = w!("MimonitorTrayWindowClass");

            unsafe extern "system" fn wnd_proc(
                hwnd: HWND,
                msg: u32,
                wparam: WPARAM,
                lparam: LPARAM,
            ) -> LRESULT {
                match msg {
                    WM_TRAY_CALLBACK => {
                        let event = lparam.0 as u32;
                        if event == WM_LBUTTONUP {
                            if let Some(ref tx) = *EVENT_SENDER.lock() {
                                let _ = tx.send(TrayEvent::ToggleBrightnessPanel);
                            }
                        } else if event == WM_RBUTTONUP {
                            let mut pt = POINT { x: 0, y: 0 };
                            let _ = GetCursorPos(&mut pt);
                            let _ = SetForegroundWindow(hwnd);

                            if let Ok(h_menu) = CreatePopupMenu() {
                                let _ =
                                    AppendMenuW(h_menu, MF_STRING, CMD_OPEN_MAIN, w!("打开主窗口"));
                                let _ = AppendMenuW(
                                    h_menu,
                                    MF_STRING,
                                    CMD_TOGGLE_BRIGHTNESS,
                                    w!("统一亮度面板"),
                                );
                                let _ = AppendMenuW(
                                    h_menu,
                                    MF_STRING,
                                    CMD_REFRESH_MONITORS,
                                    w!("刷新显示器列表"),
                                );
                                MENU_CHOICES.lock().clear();
                                let config = crate::config::load_settings();
                                let catalog = catalog::catalog();
                                for key in &config.tray_items {
                                    if let Some(entry) =
                                        catalog.iter().find(|entry| entry.id == key)
                                    {
                                        if let Ok(submenu) = CreatePopupMenu() {
                                            if let Some(idx) = crate::hotkey::AdjustParam::ALL
                                                .iter()
                                                .position(|p| p.as_str() == entry.id)
                                            {
                                                let id = 3000 + idx;
                                                let label: Vec<u16> =
                                                    "滑杆调节…".encode_utf16().chain(Some(0)).collect();
                                                let _ = AppendMenuW(
                                                    submenu,
                                                    MF_STRING,
                                                    id,
                                                    PCWSTR(label.as_ptr()),
                                                );
                                            }
                                            for (label, choice) in &entry.choices {
                                                let mut choices = MENU_CHOICES.lock();
                                                let id = 2000 + choices.len();
                                                choices.push(choice.clone());
                                                let label: Vec<u16> =
                                                    label.encode_utf16().chain(Some(0)).collect();
                                                let _ = AppendMenuW(
                                                    submenu,
                                                    MF_STRING,
                                                    id,
                                                    PCWSTR(label.as_ptr()),
                                                );
                                            }
                                            let label: Vec<u16> =
                                                entry.label.encode_utf16().chain(Some(0)).collect();
                                            let _ = AppendMenuW(
                                                h_menu,
                                                windows::Win32::UI::WindowsAndMessaging::MF_POPUP,
                                                submenu.0 as usize,
                                                PCWSTR(label.as_ptr()),
                                            );
                                        }
                                    }
                                }
                                let _ = AppendMenuW(h_menu, MF_SEPARATOR, 0, PCWSTR::null());
                                let _ = AppendMenuW(h_menu, MF_STRING, CMD_QUIT, w!("退出"));

                                let cmd = TrackPopupMenuEx(
                                    h_menu,
                                    // Avoid the Windows menu fade path, which
                                    // can leave a blank surface until hover.
                                    (TPM_RETURNCMD | TPM_NOANIMATION).0,
                                    pt.x,
                                    pt.y,
                                    hwnd,
                                    None,
                                );
                                cancel_menu_paint(hwnd);
                                // Complete notification-area menu dismissal
                                // before a subsequent click starts another menu.
                                let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                                let _ = DestroyMenu(h_menu);

                                if let Some(ref tx) = *EVENT_SENDER.lock() {
                                    match cmd.0 as usize {
                                        CMD_OPEN_MAIN => {
                                            let _ = tx.send(TrayEvent::OpenMainWindow);
                                        }
                                        CMD_TOGGLE_BRIGHTNESS => {
                                            let _ = tx.send(TrayEvent::ToggleBrightnessPanel);
                                        }
                                        CMD_REFRESH_MONITORS => {
                                            let _ = tx.send(TrayEvent::RefreshMonitors);
                                        }
                                        CMD_QUIT => {
                                            let _ = tx.send(TrayEvent::Quit);
                                        }
                                        id @ 3000..3010 => {
                                            let idx = id - 3000;
                                            if let Some(&param) = crate::hotkey::AdjustParam::ALL.get(idx) {
                                                let _ = tx.send(TrayEvent::OpenValueSlider(param));
                                            }
                                        }
                                        id if id >= 2000 => {
                                            if let Some(choice) =
                                                MENU_CHOICES.lock().get(id - 2000).cloned()
                                            {
                                                let _ = tx.send(TrayEvent::Custom(choice));
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        LRESULT(0)
                    }
                    WM_INITMENUPOPUP => {
                        // Native submenus may validate their first paint before
                        // their surface is ready. Defer one repaint past menu
                        // initialization; this also covers each newly opened submenu.
                        cancel_menu_paint(hwnd);
                        // KillTimer leaves already queued WM_TIMER messages in
                        // the queue. A new ID prevents an old popup's message
                        // from canceling a newly opened popup's repaint.
                        let timer = MENU_PAINT_SERIAL.get().wrapping_add(1).max(1);
                        MENU_PAINT_SERIAL.set(timer);
                        MENU_PAINT_TIMER.set(SetTimer(Some(hwnd), timer, 250, None));
                        DefWindowProcW(hwnd, msg, wparam, lparam)
                    }
                    WM_TIMER if wparam.0 != 0 && wparam.0 == MENU_PAINT_TIMER.get() => {
                        cancel_menu_paint(hwnd);
                        let _ = EnumThreadWindows(
                            windows::Win32::System::Threading::GetCurrentThreadId(),
                            Some(paint_visible_menu),
                            LPARAM(0),
                        );
                        LRESULT(0)
                    }
                    WM_CLOSE => {
                        let mut nid = NOTIFYICONDATAW {
                            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                            hWnd: hwnd,
                            uID: 1,
                            ..Default::default()
                        };
                        let _ = Shell_NotifyIconW(NIM_DELETE, &mut nid);
                        let _ = DestroyWindow(hwnd);
                        LRESULT(0)
                    }
                    WM_DESTROY => {
                        PostQuitMessage(0);
                        LRESULT(0)
                    }
                    _ => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
            }

            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassW(&wc);

            let hwnd_res = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class_name,
                w!("MimonitorTrayWindow"),
                WS_OVERLAPPED,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                None,
                None,
                None,
                None,
            );

            let hwnd = match hwnd_res {
                Ok(h) => h,
                Err(_) => return,
            };

            *hwnd_holder.lock() = Some(hwnd.0 as isize);

            // 初始化托盘图标
            let icon = app_icon();
            let mut tip = [0u16; 128];
            let tip_str = "Mimonitor 工具箱".encode_utf16().collect::<Vec<u16>>();
            let copy_len = tip_str.len().min(127);
            tip[..copy_len].copy_from_slice(&tip_str[..copy_len]);

            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_TRAY_CALLBACK,
                hIcon: icon,
                szTip: tip,
                ..Default::default()
            };

            let _ = Shell_NotifyIconW(NIM_ADD, &mut nid);

            // 保持 Win32 消息循环
            let mut msg = std::mem::zeroed();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            // 退出清理
            let _ = Shell_NotifyIconW(NIM_DELETE, &mut nid);
            let _ = DestroyWindow(hwnd);
            *hwnd_holder.lock() = None;
        });

        *self.worker_thread.lock() = Some(handle);
    }

    /// 关闭托盘管理器，安全发送 WM_CLOSE 并等待线程有界退出
    pub fn shutdown(&self) {
        if let Some(raw) = *self.hwnd_raw.lock() {
            unsafe {
                let hwnd = HWND(raw as *mut _);
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(h) = self.worker_thread.lock().take() {
            let _ = h.join();
        }
    }

    /// UI 线程非阻塞提取托盘消息
    pub fn try_recv_event(&self) -> Option<TrayEvent> {
        let rx = self.event_receiver.lock();
        rx.try_recv().ok()
    }
}

impl Drop for SystemTrayManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Use the same embedded icon for Explorer, taskbar, windows and tray.
fn app_icon() -> windows::Win32::UI::WindowsAndMessaging::HICON {
    unsafe {
        let module =
            windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
        LoadIconW(Some(module.into()), PCWSTR(1usize as *const u16)).unwrap_or_default()
    }
}
pub fn set_window_icon(hwnd: HWND) {
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_SETICON};
        let icon = app_icon();
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(0)),
            Some(LPARAM(icon.0 as isize)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(1)),
            Some(LPARAM(icon.0 as isize)),
        );
    }
}
