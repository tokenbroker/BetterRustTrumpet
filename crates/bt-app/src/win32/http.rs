//! One HTTPS GET through WinHTTP (D55: the update check is the app's only
//! request). Blocking: call it off the UI thread.
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::core::{HSTRING, PCWSTR};

/// Answers larger than this are not what was asked for.
const MAX_BODY: usize = 4 << 20;
const TIMEOUT_MS: i32 = 15_000;

/// A WinHTTP handle, closed on drop.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle came from WinHTTP and is closed once.
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

fn handle(raw: *mut core::ffi::c_void) -> Option<Handle> {
    (!raw.is_null()).then_some(Handle(raw))
}

/// GETs `https://{host}{path}` and returns the body of a 200 answer.
/// `headers` are extra request lines ("Name: value\r\n...").
pub fn get(host: &str, path: &str, agent: &str, headers: &str) -> Option<Vec<u8>> {
    let agent = HSTRING::from(agent);
    let host = HSTRING::from(host);
    let path = HSTRING::from(path);
    let verb = HSTRING::from("GET");
    let extra: Vec<u16> = headers.encode_utf16().collect();
    // SAFETY: every string outlives the calls that read it; each handle is
    // used only while it is open (Handle closes children before parents,
    // as they drop in reverse order); ReadData writes at most `chunk.len()`
    // bytes into `chunk`.
    unsafe {
        let session = handle(WinHttpOpen(
            &agent,
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS).ok()?;
        let connection = handle(WinHttpConnect(
            session.0,
            &host,
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        ))?;
        let request = handle(WinHttpOpenRequest(
            connection.0,
            &verb,
            &path,
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        WinHttpSendRequest(
            request.0,
            (!extra.is_empty()).then_some(&extra[..]),
            None,
            0,
            0,
            0,
        )
        .ok()?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).ok()?;
        let mut status = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut size,
            std::ptr::null_mut(),
        )
        .ok()?;
        if status != 200 {
            return None;
        }
        let mut body = Vec::new();
        let mut chunk = vec![0u8; 16 * 1024];
        loop {
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
            )
            .ok()?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > MAX_BODY {
                return None;
            }
        }
        Some(body)
    }
}
