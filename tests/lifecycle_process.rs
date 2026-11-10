//! Opt-in desktop regression tests. Each process gets a unique empty profile;
//! these tests never connect to ADB, write display settings, or touch the user's
//! running instance. Run with --ignored --test-threads=1 on Windows desktop.
#![cfg(windows)]
use std::{
    fs,
    mem::size_of,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::BOOL,
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, WPARAM},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE},
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
            PostMessageW, WM_CLOSE, WM_DISPLAYCHANGE,
        },
    },
};

struct Fixture {
    child: Child,
    directory: PathBuf,
}
impl Fixture {
    fn start(tray: bool, close: &str) -> Self {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/lifecycle-tests")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("config.json"),
            serde_json::to_vec(&serde_json::json!({
                "saved_ip": "", "presets": [], "auto_tasks": [], "close_behavior": close,
                "hotkeys": {}, "adjust_hotkeys": [], "hdr_sdr_local_dimming_enabled": false,
            }))
            .unwrap(),
        )
        .unwrap();
        let mut command = Self::command(&directory);
        if tray {
            command.arg("--tray");
        }
        let child = command.spawn().unwrap();
        Self { child, directory }
    }
    fn command(directory: &std::path::Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mimonitor_toolbox_ddc"));
        command
            .env("MIMONITOR_SETTINGS_PATH", directory.join("config.json"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000);
        command
    }
    fn activate(&self, brightness: bool) {
        let mut command = Self::command(&self.directory);
        if brightness {
            command.arg("--open-brightness");
        }
        assert!(command.spawn().unwrap().wait().unwrap().success());
    }
    fn ui_windows(&self) -> Vec<TestWindow> {
        let children = child_pids(self.child.id());
        windows()
            .into_iter()
            .filter(|w| children.contains(&w.pid) && w.class == "Zed::Window")
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Only this UUID directory below the repository's target/ is removed.
        let expected = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lifecycle-tests");
        assert!(self.directory.starts_with(expected));
        for _ in 0..30 {
            if fs::remove_dir_all(&self.directory).is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}
struct TestWindow {
    hwnd: HWND,
    pid: u32,
    class: String,
    title: String,
    visible: bool,
}
fn windows() -> Vec<TestWindow> {
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        let output = &mut *(data.0 as *mut Vec<TestWindow>);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut class = [0u16; 128];
        let length = GetClassNameW(hwnd, &mut class);
        let mut title = [0u16; 256];
        let title_length = GetWindowTextW(hwnd, &mut title);
        output.push(TestWindow {
            hwnd,
            pid,
            class: String::from_utf16_lossy(&class[..length as usize]),
            title: String::from_utf16_lossy(&title[..title_length as usize]),
            visible: IsWindowVisible(hwnd).as_bool(),
        });
        BOOL(1)
    }
    let mut output = Vec::new();
    unsafe {
        EnumWindows(Some(collect), LPARAM(&mut output as *mut _ as isize)).unwrap();
    }
    output
}
fn child_pids(parent: u32) -> Vec<u32> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).unwrap();
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut output = Vec::new();
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                if entry.th32ParentProcessID == parent {
                    output.push(entry.th32ProcessID);
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        CloseHandle(snapshot).unwrap();
        output
    }
}
fn until(label: &str, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("Timed out: {label}");
}
fn close(window: &TestWindow) {
    unsafe {
        PostMessageW(Some(window.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).unwrap();
    }
}
fn terminate(pid: u32) {
    unsafe {
        let process = OpenProcess(PROCESS_TERMINATE, false, pid).unwrap();
        TerminateProcess(process, 1).unwrap();
        CloseHandle(process).unwrap();
    }
}

#[test]
#[ignore = "requires interactive Windows desktop and launches isolated test UI"]
fn resident_has_no_gpui_and_ui_is_destroyed_reopened_and_crash_isolated() {
    let mut fixture = Fixture::start(true, "tray");
    until("resident tray ready", || {
        windows()
            .iter()
            .any(|w| w.pid == fixture.child.id() && w.class == "MimonitorTrayWindowClass")
    });
    assert!(
        child_pids(fixture.child.id()).is_empty(),
        "tray-only startup must not start GPUI"
    );
    assert!(!windows()
        .iter()
        .any(|w| w.pid == fixture.child.id() && w.class == "Zed::Window"));
    let mut previous_pid = 0;
    for _ in 0..3 {
        fixture.activate(false);
        until("main UI visible", || {
            fixture
                .ui_windows()
                .iter()
                .any(|w| w.visible && w.title == "Mimonitor 工具箱")
        });
        let main = fixture
            .ui_windows()
            .into_iter()
            .find(|w| w.title == "Mimonitor 工具箱")
            .unwrap();
        assert_ne!(
            main.pid, previous_pid,
            "closed UI must be a new process on reopening"
        );
        assert_ne!(main.pid, fixture.child.id());
        previous_pid = main.pid;
        close(&main);
        until("UI process reaped", || {
            child_pids(fixture.child.id()).is_empty()
        });
        assert!(
            fixture.child.try_wait().unwrap().is_none(),
            "closing panel must retain backend"
        );
        assert!(fixture.ui_windows().is_empty());
        // No GPUI HWND exists to be revived by display topology broadcasts.
        for window in windows()
            .into_iter()
            .filter(|w| w.pid == fixture.child.id())
        {
            unsafe {
                PostMessageW(Some(window.hwnd), WM_DISPLAYCHANGE, WPARAM(0), LPARAM(0)).unwrap();
            }
        }
        assert!(!windows()
            .iter()
            .any(|w| w.pid == fixture.child.id() && w.class == "Zed::Window"));
    }
    fixture.activate(false);
    until("main UI before crash", || !fixture.ui_windows().is_empty());
    let crashed_pid = fixture.ui_windows()[0].pid;
    // Allow the startup acknowledgement to cross the pipe before crashing UI.
    thread::sleep(Duration::from_millis(300));
    terminate(crashed_pid);
    until("crashed UI reaped", || {
        child_pids(fixture.child.id()).is_empty()
    });
    assert!(fixture.child.try_wait().unwrap().is_none());
    fixture.activate(true);
    until("brightness popup after UI crash", || {
        fixture.ui_windows().iter().any(|w| w.visible)
    });
    let ui_pid = fixture.ui_windows()[0].pid;
    let ui_process = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | windows::Win32::System::Threading::PROCESS_SYNCHRONIZE,
            false,
            ui_pid,
        )
        .unwrap()
    };
    // The job guarantees no orphan UI if the backend itself crashes.
    fixture.child.kill().unwrap();
    fixture.child.wait().unwrap();
    until("owned UI killed with backend", || unsafe {
        windows::Win32::System::Threading::WaitForSingleObject(ui_process, 0)
            == windows::Win32::Foundation::WAIT_OBJECT_0
    });
    unsafe {
        CloseHandle(ui_process).unwrap();
    }
}

#[test]
#[ignore = "requires interactive Windows desktop and launches isolated test UI"]
fn close_behavior_exit_stops_backend_and_ui() {
    let mut fixture = Fixture::start(false, "exit");
    until("main UI visible", || {
        fixture
            .ui_windows()
            .iter()
            .any(|w| w.visible && w.title == "Mimonitor 工具箱")
    });
    close(
        &fixture
            .ui_windows()
            .into_iter()
            .find(|w| w.title == "Mimonitor 工具箱")
            .unwrap(),
    );
    until("backend exits", || {
        fixture.child.try_wait().unwrap().is_some()
    });
    assert!(fixture.ui_windows().is_empty());
}
