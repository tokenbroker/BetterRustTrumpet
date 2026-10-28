//! The client's Win32 calls: the session id and the pipe owner check
//! (`PipeOptions.CurrentUserOnly` on the client side).
use std::{fs::File, os::windows::io::AsRawHandle};
use windows::Win32::{
    Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree},
    Security::{
        Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT},
        EqualSid, GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{GetCurrentProcess, GetCurrentProcessId, OpenProcessToken},
    },
};

pub const SECURITY_IDENTIFICATION: u32 =
    windows::Win32::Storage::FileSystem::SECURITY_IDENTIFICATION.0;

pub fn session_id() -> u32 {
    let mut id = 0;
    // SAFETY: writes the session id of this process into `id`.
    let _ = unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut id) };
    id
}

/// The current user's TOKEN_USER, in a u64-aligned buffer.
fn token_user() -> Option<Vec<u64>> {
    // SAFETY: the token handle is closed after use; the buffer is sized by
    // the first call and aligned for TOKEN_USER.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
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
        result.ok()?;
        Some(buffer)
    }
}

/// Whether the pipe's owner is the current user.
pub fn owned_by_current_user(pipe: &File) -> bool {
    let Some(user) = token_user() else {
        return false;
    };
    // SAFETY: `user` holds a TOKEN_USER filled in above; `owner` points into
    // `descriptor`, which is freed with LocalFree after the comparison.
    unsafe {
        let user = &*user.as_ptr().cast::<TOKEN_USER>();
        let mut owner = PSID::default();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let status = GetSecurityInfo(
            HANDLE(pipe.as_raw_handle()),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            Some(&mut descriptor),
        );
        if status != ERROR_SUCCESS {
            return false;
        }
        let same = EqualSid(owner, user.User.Sid).is_ok();
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        same
    }
}
