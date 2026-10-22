//! One launcher at a time (docs/DESIGN.md 9.5).
//!
//! The first launcher takes a named mutex and serves a named pipe. A later launch, with or without
//! a link, writes its arguments to that pipe, waits for the answer and exits; the running launcher
//! brings its window forward and acts on them. Nothing here looks at what the arguments mean.
//!
//! The pipe is per user and refuses remote clients; only the user's own processes can write to it.
//! Previews take no part (see `main`); a test run names its own instance with `ANYPS5_INSTANCE`.

use std::sync::mpsc;
use std::sync::{Arc, OnceLock};

/// The arguments of one later launch.
pub type Arguments = Vec<String>;

/// What joining the single-instance group came to.
pub enum Join {
    /// This is the launcher; later launches arrive in the inbox.
    Primary(Inbox),
    /// Another launcher took the arguments. Exit.
    Delivered,
    /// No other launcher answered (or this system has no handoff). Start alone.
    Alone,
}

/// Arguments of later launches, handed over by the pipe thread.
pub struct Inbox {
    receiver: mpsc::Receiver<Arguments>,
    repaint: Arc<OnceLock<egui::Context>>,
}

impl Inbox {
    /// Wakes the window when something arrives.
    pub fn attach(&self, ctx: &egui::Context) {
        let _ = self.repaint.set(ctx.clone());
    }

    /// The launches that arrived since the last call, oldest first.
    pub fn drain(&self) -> Vec<Arguments> {
        self.receiver.try_iter().collect()
    }
}

/// The instance name: fixed for users, replaced by `ANYPS5_INSTANCE` for test runs.
pub fn name() -> String {
    match std::env::var("ANYPS5_INSTANCE") {
        Ok(name) if !name.is_empty() => format!("test-{}", sanitize(&name)),
        _ => "main".to_owned(),
    }
}

/// True when a test run chose its own instance name.
pub fn is_named() -> bool {
    name() != "main"
}

fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(32)
        .collect()
}

/// The message of a launch: a greeting and the arguments, separated by NUL. Arguments are cut to
/// the link length limit and counted, so a message always fits one pipe read.
pub fn encode(arguments: &[String]) -> Vec<u8> {
    let mut parts = vec!["anyps5-launcher/1".to_owned()];
    for argument in arguments.iter().take(MAX_ARGUMENTS) {
        let end = (0..=argument.len().min(anyps5_core::links::MAX_LEN))
            .rev()
            .find(|i| argument.is_char_boundary(*i))
            .unwrap_or(0);
        parts.push(argument[..end].replace('\0', ""));
    }
    parts.join("\0").into_bytes()
}

/// The arguments of a message, or `None` when it is not a launch.
pub fn decode(message: &[u8]) -> Option<Arguments> {
    let text = std::str::from_utf8(message).ok()?;
    let mut parts = text.split('\0');
    if parts.next()? != "anyps5-launcher/1" {
        return None;
    }
    Some(parts.take(MAX_ARGUMENTS).map(str::to_owned).collect())
}

const MAX_ARGUMENTS: usize = 8;
/// A message is read in one go: eight arguments of the longest length, with room to spare.
const MESSAGE_BYTES: usize = 8 * (anyps5_core::links::MAX_LEN + 1) + 64;
const ANSWER: &[u8] = b"ok";

#[cfg(windows)]
pub use windows_impl::join;

#[cfg(not(windows))]
pub fn join(_name: &str, _arguments: &[String]) -> Join {
    Join::Alone
}

#[cfg(windows)]
mod windows_impl {
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Write};
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use std::sync::mpsc;
    use std::sync::{Arc, OnceLock};
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{
        ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, GetLastError, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };
    use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongW,
        GetWindowThreadProcessId, IsIconic, IsWindowVisible, SW_RESTORE, SetForegroundWindow,
        ShowWindow, WS_EX_TOOLWINDOW,
    };
    use windows_sys::core::BOOL;

    use super::{ANSWER, Arguments, Inbox, Join, MESSAGE_BYTES, decode, encode};

    /// How long a later launch waits for the running one to take its message.
    const HANDOFF: Duration = Duration::from_secs(3);
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_PIPE_BUSY: i32 = 231;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn pipe_path(name: &str) -> String {
        let user = std::env::var("USERNAME").unwrap_or_default();
        format!(r"\\.\pipe\AnyPS5Launcher-{name}-{}", super::sanitize(&user))
    }

    /// Becomes the launcher, or hands `arguments` to the one that is running.
    pub fn join(name: &str, arguments: &[String]) -> Join {
        let mutex = wide(&format!("Local\\AnyPS5Launcher-{name}"));
        // SAFETY: a NUL-terminated name; the handle is kept for the life of the process, which is
        // what holds the name.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, mutex.as_ptr()) };
        // SAFETY: reads the thread's last error right after the call.
        let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        if handle.is_null() {
            tracing::warn!("single instance: cannot create the mutex");
            return Join::Alone;
        }
        let path = pipe_path(name);
        if !exists {
            return match serve(&path) {
                Some(inbox) => Join::Primary(inbox),
                None => Join::Alone,
            };
        }
        match deliver(&path, arguments) {
            Ok(()) => Join::Delivered,
            Err(error) => {
                tracing::warn!("single instance: cannot reach the running launcher: {error}");
                Join::Alone
            }
        }
    }

    /// Writes the message and waits for the answer, for up to [`HANDOFF`]. A launcher that is still
    /// starting has no pipe yet, one that is busy has none free: both are tried again.
    fn deliver(path: &str, arguments: &[String]) -> std::io::Result<()> {
        let (sender, receiver) = mpsc::channel();
        let path = path.to_owned();
        let message = encode(arguments);
        std::thread::spawn(move || {
            let _ = sender.send(try_deliver(&path, &message));
        });
        receiver
            .recv_timeout(HANDOFF)
            .unwrap_or_else(|_| Err(std::io::ErrorKind::TimedOut.into()))
    }

    fn try_deliver(path: &str, message: &[u8]) -> std::io::Result<()> {
        let started = Instant::now();
        loop {
            match OpenOptions::new().read(true).write(true).open(path) {
                Ok(mut pipe) => {
                    // The running launcher may take the window to the front; this process was just
                    // started by the user and so is allowed to say so.
                    // SAFETY: a plain call; `u32::MAX` is ASFW_ANY.
                    unsafe { AllowSetForegroundWindow(u32::MAX) };
                    pipe.write_all(message)?;
                    let mut answer = [0u8; 8];
                    let read = pipe.read(&mut answer)?;
                    return if &answer[..read] == ANSWER {
                        Ok(())
                    } else {
                        Err(std::io::ErrorKind::InvalidData.into())
                    };
                }
                Err(error)
                    if matches!(
                        error.raw_os_error(),
                        Some(ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY)
                    ) && started.elapsed() < HANDOFF =>
                {
                    std::thread::sleep(Duration::from_millis(40));
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Creates the pipe and answers on it in a thread for as long as the process lives.
    fn serve(path: &str) -> Option<Inbox> {
        let (sender, receiver) = mpsc::channel::<Arguments>();
        let repaint: Arc<OnceLock<egui::Context>> = Arc::default();
        // The first instance proves nobody else owns the name; it is made here so a failure is
        // known before the window opens.
        let first = create_pipe(path, true)?;
        let thread_repaint = repaint.clone();
        let thread_path = path.to_owned();
        std::thread::Builder::new()
            .name("single-instance".into())
            .spawn(move || {
                let mut next = Some(first);
                loop {
                    let Some(pipe) = next.take().or_else(|| create_pipe(&thread_path, false)) else {
                        tracing::warn!("single instance: the pipe is gone");
                        return;
                    };
                    // SAFETY: a handle made by `CreateNamedPipeW`, no overlapped I/O.
                    let connected = unsafe { ConnectNamedPipe(pipe as _, std::ptr::null_mut()) } != 0
                        // SAFETY: the thread's last error.
                        || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
                    // SAFETY: the handle is ours and from here on owned by the `File`.
                    let file = unsafe { File::from_raw_handle(pipe as RawHandle) };
                    if !connected {
                        continue;
                    }
                    let sender = sender.clone();
                    let repaint = thread_repaint.clone();
                    // One thread per client: a client that never writes holds only its own.
                    let _ = std::thread::Builder::new()
                        .name("single-instance-client".into())
                        .spawn(move || answer(file, &sender, &repaint));
                }
            })
            .ok()?;
        Some(Inbox { receiver, repaint })
    }

    /// The pipe handle as a number, so it can move to the serving thread.
    fn create_pipe(path: &str, first: bool) -> Option<usize> {
        let name = wide(path);
        let open_mode = PIPE_ACCESS_DUPLEX
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // SAFETY: a NUL-terminated name; default security means only this user can write.
        let handle = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open_mode,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64,
                MESSAGE_BYTES as u32,
                0,
                std::ptr::null(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            // SAFETY: the thread's last error.
            tracing::warn!(
                "single instance: cannot create the pipe (error {})",
                unsafe { GetLastError() }
            );
            None
        } else {
            Some(handle as usize)
        }
    }

    /// Restores and focuses the launcher's window: visible, not owned, not a tool window (the
    /// trophy banner is one).
    fn bring_to_front() {
        unsafe extern "system" fn each(window: HWND, found: LPARAM) -> BOOL {
            // SAFETY: `found` is the address of the `Option<HWND>` below, alive during the call.
            unsafe {
                let mut pid = 0;
                GetWindowThreadProcessId(window, &mut pid);
                let own = pid == GetCurrentProcessId()
                    && IsWindowVisible(window) != 0
                    && GetWindow(window, GW_OWNER).is_null()
                    && GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW == 0;
                if own {
                    *(found as *mut Option<HWND>) = Some(window);
                    return 0;
                }
            }
            1
        }
        let mut found: Option<HWND> = None;
        // SAFETY: the callback only runs inside the call and only writes `found`.
        unsafe {
            EnumWindows(Some(each), &mut found as *mut Option<HWND> as LPARAM);
            if let Some(window) = found {
                if IsIconic(window) != 0 {
                    ShowWindow(window, SW_RESTORE);
                }
                SetForegroundWindow(window);
            }
        }
    }

    /// Reads one message, passes it on, answers.
    fn answer(
        mut pipe: File,
        sender: &mpsc::Sender<Arguments>,
        repaint: &Arc<OnceLock<egui::Context>>,
    ) {
        let mut buffer = vec![0u8; MESSAGE_BYTES];
        let Ok(read) = pipe.read(&mut buffer) else {
            return;
        };
        let reply: &[u8] = match decode(&buffer[..read]) {
            Some(arguments) => {
                tracing::info!("single instance: another launch handed over {arguments:?}");
                // A minimised window draws no frames and so would never act on the message.
                bring_to_front();
                let _ = sender.send(arguments);
                if let Some(ctx) = repaint.get() {
                    ctx.request_repaint();
                }
                ANSWER
            }
            None => b"no",
        };
        // The answer is flushed so it is still there when the client reads it.
        if pipe.write_all(reply).is_ok() {
            let _ = pipe.sync_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_round_trips() {
        let arguments = vec![
            "anyps5://game/PPSA16064".to_owned(),
            r"D:\Dumps\Game".to_owned(),
        ];
        assert_eq!(decode(&encode(&arguments)), Some(arguments));
        assert_eq!(decode(&encode(&[])), Some(Vec::new()));
    }

    #[test]
    fn foreign_messages_are_not_launches() {
        assert_eq!(decode(b""), None);
        assert_eq!(decode(b"hello\0anyps5://settings"), None);
        assert_eq!(decode(&[0xff, 0xfe]), None);
    }

    #[test]
    fn messages_stay_small() {
        let long = "x".repeat(10_000);
        let many: Vec<String> = (0..20).map(|_| long.clone()).collect();
        let message = encode(&many);
        assert!(message.len() <= MESSAGE_BYTES);
        let arguments = decode(&message).unwrap();
        assert_eq!(arguments.len(), MAX_ARGUMENTS);
        assert!(
            arguments
                .iter()
                .all(|a| a.len() <= anyps5_core::links::MAX_LEN)
        );
    }

    #[test]
    fn cutting_keeps_characters_whole() {
        let text = "ä".repeat(anyps5_core::links::MAX_LEN);
        let arguments = decode(&encode(&[text])).unwrap();
        assert!(arguments[0].len() <= anyps5_core::links::MAX_LEN);
        assert!(arguments[0].chars().all(|c| c == 'ä'));
    }

    #[test]
    fn instance_names_are_safe() {
        assert_eq!(sanitize("a b\\c"), "a_b_c");
        assert_eq!(sanitize(&"x".repeat(100)).len(), 32);
    }

    /// Two launches of one name: the first becomes the launcher, the second hands over and is
    /// delivered. Windows only; the name is unique to the test process.
    #[cfg(windows)]
    #[test]
    fn the_second_launch_reaches_the_first() {
        let name = format!("unit-{}", std::process::id());
        let Join::Primary(inbox) = join(&name, &[]) else {
            panic!("the first launch should be the primary");
        };
        let sent = vec!["anyps5://settings/about".to_owned()];
        assert!(matches!(join(&name, &sent), Join::Delivered));
        assert!(matches!(join(&name, &[]), Join::Delivered));
        let received = inbox.drain();
        assert_eq!(received, vec![sent, Vec::new()]);
    }
}
