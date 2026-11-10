//! Keep UI IPC endpoints out of process-global stdio and external applications.
use std::{
    ffi::{OsStr, OsString},
    fs::{File, OpenOptions},
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::{CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE},
        Globalization::{CompareStringOrdinal, CSTR_EQUAL, CSTR_LESS_THAN},
        System::{
            Console::{GetStdHandle, SetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
            Threading::{
                CreateProcessW, GetCurrentProcess, CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT,
                PROCESS_INFORMATION, STARTUPINFOW,
            },
        },
    },
};

static NULL_STDIO: std::sync::OnceLock<File> = std::sync::OnceLock::new();

/// The inherited endpoints are duplicated with inheritance disabled, then
/// removed from global stdio. Libraries and future Command::spawn helpers can
/// neither corrupt the protocol nor keep an endpoint open after the UI exits.
pub(crate) fn take_ui_pipes() -> io::Result<(File, File)> {
    unsafe {
        let input = GetStdHandle(STD_INPUT_HANDLE).map_err(io::Error::other)?;
        let output = GetStdHandle(STD_OUTPUT_HANDLE).map_err(io::Error::other)?;
        let duplicate = |handle: HANDLE| -> io::Result<File> {
            let mut private = HANDLE::default();
            DuplicateHandle(
                GetCurrentProcess(),
                handle,
                GetCurrentProcess(),
                &mut private,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
            .map_err(io::Error::other)?;
            Ok(File::from_raw_handle(private.0))
        };
        let private_input = duplicate(input)?;
        let private_output = duplicate(output)?;
        if NULL_STDIO.get().is_none() {
            let null = OpenOptions::new().read(true).write(true).open("NUL")?;
            let _ = NULL_STDIO.set(null);
        }
        let null = NULL_STDIO.get().expect("NUL stdio initialized");
        SetStdHandle(STD_INPUT_HANDLE, HANDLE(null.as_raw_handle())).map_err(io::Error::other)?;
        SetStdHandle(STD_OUTPUT_HANDLE, HANDLE(null.as_raw_handle())).map_err(io::Error::other)?;
        CloseHandle(input).map_err(io::Error::other)?;
        if output != input {
            CloseHandle(output).map_err(io::Error::other)?;
        }
        Ok((private_input, private_output))
    }
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn environment(overrides: &[(&str, OsString)]) -> Vec<u16> {
    let mut vars: Vec<_> = std::env::vars_os().collect();
    for (key, value) in overrides {
        vars.retain(|(old, _)| !old.to_string_lossy().eq_ignore_ascii_case(key));
        vars.push((OsString::from(key), value.clone()));
    }
    vars.sort_by(|(a, _), (b, _)| {
        let a: Vec<_> = a.encode_wide().collect();
        let b: Vec<_> = b.encode_wide().collect();
        match unsafe { CompareStringOrdinal(&a, &b, true) } {
            CSTR_LESS_THAN => std::cmp::Ordering::Less,
            CSTR_EQUAL => std::cmp::Ordering::Equal,
            _ => std::cmp::Ordering::Greater,
        }
    });
    let mut block = Vec::new();
    for (key, value) in vars {
        block.extend(key.encode_wide());
        block.push('=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

fn create_console(
    executable: &Path,
    command_line: &str,
    directory: &Path,
    overrides: &[(&str, OsString)],
    hidden: bool,
) -> Result<PROCESS_INFORMATION, String> {
    let executable = wide(executable.as_os_str());
    let mut line = wide(OsStr::new(command_line));
    let directory = wide(directory.as_os_str());
    let environment = environment(overrides);
    let mut startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    if hidden {
        startup.dwFlags = windows::Win32::System::Threading::STARTF_USESHOWWINDOW;
        startup.wShowWindow = 0;
    }
    let mut process = PROCESS_INFORMATION::default();
    unsafe {
        // Do not set STARTF_USESTDHANDLES. Windows must allocate interactive
        // console handles, rather than inherit UI pipes or NUL handles.
        CreateProcessW(
            PCWSTR(executable.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
            Some(environment.as_ptr().cast()),
            PCWSTR(directory.as_ptr()),
            &startup,
            &mut process,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(process)
}

pub(crate) fn open_adb_console(adb: &Path, serial: &str, shell: bool) -> Result<(), String> {
    let directory = adb.parent().ok_or("ADB 所在目录无效")?;
    let executable = std::path::PathBuf::from(
        std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
    )
    .join("System32/cmd.exe");
    let script = if shell {
        r#""%MIMONITOR_ADB_BIN%" -P %MIMONITOR_ADB_PORT% -s %MIMONITOR_ADB_SERIAL% shell"#
    } else {
        r#"doskey adb=adb.exe -P %MIMONITOR_ADB_PORT% $*"#
    };
    let process = create_console(
        &executable,
        &format!("\"{}\" /k {}", executable.display(), script),
        directory,
        &[
            ("MIMONITOR_ADB_BIN", adb.as_os_str().to_os_string()),
            (
                "MIMONITOR_ADB_PORT",
                crate::adb::get_adb_server_port().to_string().into(),
            ),
            ("MIMONITOR_ADB_SERIAL", serial.into()),
        ],
        false,
    )?;
    unsafe {
        let _ = CloseHandle(process.hThread);
        let _ = CloseHandle(process.hProcess);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        sync::mpsc,
        time::Duration,
    };
    const PROBE: &str = "MIMONITOR_STDIO_PROBE";
    #[test]
    #[ignore = "only executed in an isolated child process by the regression test"]
    fn stdio_probe_child() {
        let directory = std::path::PathBuf::from(std::env::var_os(PROBE).expect("probe directory"));
        match std::env::var("MIMONITOR_STDIO_ROLE").unwrap().as_str() {
            "holder" => {
                while !directory.join("release").exists() {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            "ui" => {
                let (_input, mut output) = take_ui_pipes().unwrap();
                let helper = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "process_io::tests::stdio_probe_child",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("MIMONITOR_STDIO_ROLE", "holder")
                    .creation_flags(0x08000000)
                    .spawn()
                    .unwrap();
                std::fs::write(directory.join("holder.pid"), helper.id().to_string()).unwrap();
                output.write_all(b"PRIVATE_PIPE_OK").unwrap();
                // Deliberately leave the external helper alive after UI exit.
            }
            _ => panic!("unknown probe role"),
        }
    }
    #[test]
    fn ui_pipes_close_even_when_an_external_helper_outlives_ui() {
        let directory =
            std::env::temp_dir().join(format!("mimonitor-stdio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut ui = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process_io::tests::stdio_probe_child",
                "--ignored",
                "--nocapture",
            ])
            .env(PROBE, &directory)
            .env("MIMONITOR_STDIO_ROLE", "ui")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        let mut output = ui.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = output.read_to_end(&mut bytes);
            let _ = tx.send((result, bytes));
        });
        assert!(ui.wait().unwrap().success());
        let result = rx.recv_timeout(Duration::from_secs(2));
        // Release and join the holder even if EOF was blocked, so a failing
        // test never leaves test processes or pipe-reader threads behind.
        unsafe {
            use windows::Win32::System::Threading::{
                OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
                PROCESS_TERMINATE,
            };
            let pid: u32 = std::fs::read_to_string(directory.join("holder.pid"))
                .unwrap()
                .parse()
                .unwrap();
            // Acquire the exact live holder before releasing it, avoiding PID reuse.
            let holder = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, pid).unwrap();
            std::fs::write(directory.join("release"), b"").unwrap();
            if WaitForSingleObject(holder, 3000) != windows::Win32::Foundation::WAIT_OBJECT_0 {
                let _ = TerminateProcess(holder, 1);
                let _ = WaitForSingleObject(holder, 1000);
            }
            let _ = CloseHandle(holder);
        }
        let _ = reader.join();
        let (read, bytes) = result.expect("UI pipe held open by external helper");
        read.unwrap();
        assert!(bytes
            .windows(b"PRIVATE_PIPE_OK".len())
            .any(|w| w == b"PRIVATE_PIPE_OK"));
        for _ in 0..100 {
            if std::fs::remove_dir_all(&directory).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("could not clean stdio fixture");
    }
    #[test]
    fn interactive_console_gets_real_console_handles_without_inheritance() {
        let directory =
            std::env::temp_dir().join(format!("mimonitor-console-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        // Use a console-subsystem program. The application's own test EXE is
        // built with windows_subsystem=windows and is not a console executable.
        let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let process = create_console(
            &executable,
            &format!(
                "\"{}\" -NoProfile -NonInteractive -Command \"[IO.File]::WriteAllText(($env:MIMONITOR_STDIO_PROBE + '\\console.json'), ([Console]::IsInputRedirected.ToString() + ',' + [Console]::IsOutputRedirected.ToString()))\"",
                executable.display()
            ),
            &directory,
            &[
                (PROBE, directory.as_os_str().to_os_string()),
            ],
            true,
        )
        .unwrap();
        unsafe {
            use windows::Win32::{
                Foundation::WAIT_OBJECT_0,
                System::Threading::{TerminateProcess, WaitForSingleObject},
            };
            let status = WaitForSingleObject(process.hProcess, 5000);
            if status != WAIT_OBJECT_0 {
                let _ = TerminateProcess(process.hProcess, 1);
            }
            let _ = CloseHandle(process.hThread);
            let _ = CloseHandle(process.hProcess);
            assert_eq!(status, WAIT_OBJECT_0);
        }
        let report = std::fs::read_to_string(directory.join("console.json")).unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        assert_eq!(report, "False,False");
    }
}
