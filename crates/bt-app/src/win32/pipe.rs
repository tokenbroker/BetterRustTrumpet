//! The CLI's named pipe server (PipeServer.cs): a background thread serving
//! one connection at a time, a UTF-8 command line in and one JSON line out.
//! Like the original's `PipeOptions.CurrentUserOnly`, the pipe is owned by
//! and only open to the current user, and remote clients are rejected.
use bt_core::protocol;
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL,
            INVALID_HANDLE_VALUE, LocalFree,
        },
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
                SDDL_REVISION_1,
            },
            GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX},
        System::{
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_TYPE_BYTE, PIPE_WAIT,
            },
            RemoteDesktop::ProcessIdToSessionId,
            Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcessToken},
        },
    },
    core::{HSTRING, PWSTR},
};

/// This Windows session's pipe name: each logged-on user gets their own.
pub fn session_pipe_name() -> String {
    let mut id = 0;
    // SAFETY: writes the session id of this process into `id`.
    let _ = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut id) };
    protocol::rust_pipe_name(id)
}

fn path(name: &str) -> String {
    format!(r"\\.\pipe\{name}")
}

/// The current user's SID in SDDL form.
fn user_sid() -> io::Result<String> {
    // SAFETY: the token handle is closed after use; the TOKEN_USER buffer is
    // u64-aligned and sized by the first call; the SID string is freed with
    // LocalFree as ConvertSidToStringSidW requires.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(io::Error::other)?;
        let mut len = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut len);
        let mut buffer = vec![0u64; (len as usize).div_ceil(8)];
        let result = GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &mut len,
        );
        let _ = CloseHandle(token);
        result.map_err(io::Error::other)?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text).map_err(io::Error::other)?;
        let sid = text.to_string();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        sid.map_err(io::Error::other)
    }
}

/// Owner and only allowed user: the current user (full control).
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn current_user() -> io::Result<Self> {
        let sid = user_sid()?;
        let sddl = HSTRING::from(format!("O:{sid}D:P(A;;FA;;;{sid})"));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: parses a valid SDDL string into a LocalAlloc'd descriptor,
        // freed in Drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &sddl,
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(io::Error::other)?;
        Ok(Self(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor came from LocalAlloc and is freed once.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}

/// One listening instance; a second server with the same name fails
/// (FILE_FLAG_FIRST_PIPE_INSTANCE, one instance like the original).
fn create(path: &str, descriptor: &Descriptor) -> io::Result<OwnedHandle> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    // SAFETY: the name and attributes outlive the call; a valid handle is
    // owned by the returned OwnedHandle.
    unsafe {
        let handle = CreateNamedPipeW(
            &HSTRING::from(path),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            0,
            0,
            Some(&attributes),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(OwnedHandle::from_raw_handle(handle.0))
    }
}

/// Blocks until a client connects.
fn accept(pipe: &OwnedHandle) -> io::Result<()> {
    // SAFETY: `pipe` is a live pipe handle opened without overlapped I/O.
    match unsafe { ConnectNamedPipe(HANDLE(pipe.as_raw_handle()), None) } {
        Ok(()) => Ok(()),
        // The client connected between create and connect.
        Err(e) if e.code() == ERROR_PIPE_CONNECTED.to_hresult() => Ok(()),
        Err(e) => Err(io::Error::other(e)),
    }
}

/// Reads one command and writes the handler's answer. An empty command
/// gets no answer; a panicking handler answers `{"error":…}`.
pub fn serve(
    pipe: &mut (impl io::Read + Write),
    handler: &dyn Fn(&str) -> String,
) -> io::Result<()> {
    let command = protocol::read_line_utf8(pipe)?;
    if command.is_empty() {
        return Ok(());
    }
    let response = catch_unwind(AssertUnwindSafe(|| handler(&command))).unwrap_or_else(|panic| {
        let message = panic
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "internal error".into());
        bt_core::cli_dispatch::error(message)
    });
    pipe.write_all(&protocol::encode_response(&response))
}

/// Opens the pipe as a client, waiting up to `timeout` while it does not
/// exist yet or is busy (NamedPipeClientStream.Connect).
fn open(path: &str, timeout: Duration) -> io::Result<File> {
    let deadline = Instant::now() + timeout;
    loop {
        match OpenOptions::new().read(true).write(true).open(path) {
            Ok(file) => return Ok(file),
            Err(e)
                if (e.kind() == io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32))
                    && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(e),
        }
    }
}

pub struct PipeServer {
    path: String,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl PipeServer {
    /// Serves `name` on a background thread ("BT-PipeServer") until stopped.
    /// `handler` runs on that thread.
    pub fn start(
        name: &str,
        handler: impl Fn(&str) -> String + Send + 'static,
    ) -> io::Result<Self> {
        let path = path(name);
        let running = Arc::new(AtomicBool::new(true));
        let thread = thread::Builder::new().name("BT-PipeServer".into()).spawn({
            let (path, running) = (path.clone(), running.clone());
            move || {
                let Ok(descriptor) = Descriptor::current_user() else {
                    return;
                };
                while running.load(Ordering::SeqCst) {
                    let pipe = match create(&path, &descriptor) {
                        Ok(pipe) => pipe,
                        Err(_) => {
                            // Name taken or out of resources: try again later.
                            thread::sleep(Duration::from_secs(1));
                            continue;
                        }
                    };
                    if accept(&pipe).is_err() {
                        continue;
                    }
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    let _ = serve(&mut File::from(pipe), &handler);
                }
            }
        })?;
        Ok(Self {
            path,
            running,
            thread: Some(thread),
        })
    }

    /// Stops after the current request: connects to itself for up to
    /// 500 ms and sends LF so the waiting thread wakes up.
    pub fn stop(&mut self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            return;
        }
        let timeout = Duration::from_millis(protocol::STOP_CONNECT_TIMEOUT_MS.into());
        let woke = open(&self.path, timeout)
            .and_then(|mut pipe| pipe.write_all(b"\n"))
            .is_ok();
        if let Some(thread) = self.thread.take().filter(|_| woke) {
            let _ = thread.join();
        }
    }
}

impl Drop for PipeServer {
    fn drop(&mut self) {
        self.stop();
    }
}
