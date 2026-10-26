//! Blocking HTTPS GET through WinHTTP. The DLL is loaded on first use instead
//! of being imported, so it never enters the process while every feature that
//! needs the network is off. Once loaded it stays: unloading WinHTTP while its
//! internal thread pool winds down is not safe.

use std::ffi::c_void;
use std::sync::OnceLock;

use windows::core::{s, w, HSTRING, PCSTR, PCWSTR};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};

type Handle = *mut c_void;

const ACCESS_TYPE_DEFAULT_PROXY: u32 = 0;
const ACCESS_TYPE_AUTOMATIC_PROXY: u32 = 4;
const FLAG_SECURE: u32 = 0x0080_0000;
const OPTION_DECOMPRESSION: u32 = 118;
const DECOMPRESSION_ALL: u32 = 3;
const QUERY_STATUS_CODE: u32 = 19;
const QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
const HTTPS_PORT: u16 = 443;
/// Answers larger than this are not lyrics: give up instead of buffering them.
const MAX_BODY: usize = 1 << 20;

struct Api {
    open: unsafe extern "system" fn(PCWSTR, u32, PCWSTR, PCWSTR, u32) -> Handle,
    set_timeouts: unsafe extern "system" fn(Handle, i32, i32, i32, i32) -> i32,
    set_option: unsafe extern "system" fn(Handle, u32, *const c_void, u32) -> i32,
    connect: unsafe extern "system" fn(Handle, PCWSTR, u16, u32) -> Handle,
    open_request: unsafe extern "system" fn(
        Handle,
        PCWSTR,
        PCWSTR,
        PCWSTR,
        PCWSTR,
        *const PCWSTR,
        u32,
    ) -> Handle,
    send_request:
        unsafe extern "system" fn(Handle, PCWSTR, u32, *const c_void, u32, u32, usize) -> i32,
    receive_response: unsafe extern "system" fn(Handle, *mut c_void) -> i32,
    query_headers:
        unsafe extern "system" fn(Handle, u32, PCWSTR, *mut c_void, *mut u32, *mut u32) -> i32,
    read_data: unsafe extern "system" fn(Handle, *mut c_void, u32, *mut u32) -> i32,
    close: unsafe extern "system" fn(Handle) -> i32,
}

type Proc = unsafe extern "system" fn() -> isize;

/// Casts an export to the signature documented for it. `T` must be a function pointer.
unsafe fn cast<T>(proc: Proc) -> T {
    debug_assert_eq!(size_of::<T>(), size_of::<Proc>());
    unsafe { std::mem::transmute_copy::<Proc, T>(&proc) }
}

impl Api {
    /// Loads WinHTTP the first time it is asked for; later calls reuse it.
    fn get() -> Option<&'static Self> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(Self::load).as_ref()
    }

    fn load() -> Option<Self> {
        unsafe {
            let dll = LoadLibraryExW(w!("winhttp.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
            let sym = |name: PCSTR| GetProcAddress(dll, name);
            Some(Self {
                open: cast(sym(s!("WinHttpOpen"))?),
                set_timeouts: cast(sym(s!("WinHttpSetTimeouts"))?),
                set_option: cast(sym(s!("WinHttpSetOption"))?),
                connect: cast(sym(s!("WinHttpConnect"))?),
                open_request: cast(sym(s!("WinHttpOpenRequest"))?),
                send_request: cast(sym(s!("WinHttpSendRequest"))?),
                receive_response: cast(sym(s!("WinHttpReceiveResponse"))?),
                query_headers: cast(sym(s!("WinHttpQueryHeaders"))?),
                read_data: cast(sym(s!("WinHttpReadData"))?),
                close: cast(sym(s!("WinHttpCloseHandle"))?),
            })
        }
    }
}

/// A session with one HTTPS host. Requests through it reuse the connection.
pub struct Client {
    api: &'static Api,
    session: Handle,
    connection: Handle,
}

impl Client {
    pub fn new(agent: &str, host: &str) -> Option<Self> {
        let api = Api::get()?;
        let agent = HSTRING::from(agent);
        unsafe {
            let null = PCWSTR::null();
            // Automatic proxy needs Windows 8.1; older systems take the default.
            let mut session = (api.open)(
                PCWSTR(agent.as_ptr()),
                ACCESS_TYPE_AUTOMATIC_PROXY,
                null,
                null,
                0,
            );
            if session.is_null() {
                session = (api.open)(
                    PCWSTR(agent.as_ptr()),
                    ACCESS_TYPE_DEFAULT_PROXY,
                    null,
                    null,
                    0,
                );
            }
            if session.is_null() {
                return None;
            }
            (api.set_timeouts)(session, 5_000, 5_000, 5_000, 8_000);
            let flags = DECOMPRESSION_ALL;
            (api.set_option)(session, OPTION_DECOMPRESSION, (&raw const flags).cast(), 4);
            let host = HSTRING::from(host);
            let connection = (api.connect)(session, PCWSTR(host.as_ptr()), HTTPS_PORT, 0);
            if connection.is_null() {
                (api.close)(session);
                return None;
            }
            Some(Self {
                api,
                session,
                connection,
            })
        }
    }

    /// Status code and body of `GET path`, or `None` if the request failed.
    pub fn get(&self, path: &str) -> Option<(u32, Vec<u8>)> {
        let api = self.api;
        let path = HSTRING::from(path);
        unsafe {
            let null = PCWSTR::null();
            let request = (api.open_request)(
                self.connection,
                w!("GET"),
                PCWSTR(path.as_ptr()),
                null,
                null,
                std::ptr::null(),
                FLAG_SECURE,
            );
            if request.is_null() {
                return None;
            }
            let result = self.exchange(request);
            (api.close)(request);
            result
        }
    }

    unsafe fn exchange(&self, request: Handle) -> Option<(u32, Vec<u8>)> {
        let api = self.api;
        let null = PCWSTR::null();
        unsafe {
            if (api.send_request)(request, null, 0, std::ptr::null(), 0, 0, 0) == 0
                || (api.receive_response)(request, std::ptr::null_mut()) == 0
            {
                return None;
            }
            let mut status = 0u32;
            let mut size = 4u32;
            let queried = (api.query_headers)(
                request,
                QUERY_STATUS_CODE | QUERY_FLAG_NUMBER,
                null,
                (&raw mut status).cast(),
                &mut size,
                std::ptr::null_mut(),
            );
            if queried == 0 {
                return None;
            }
            let mut body = Vec::new();
            let mut chunk = vec![0u8; 16 * 1024];
            loop {
                let mut read = 0u32;
                let ok = (api.read_data)(
                    request,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut read,
                );
                if ok == 0 {
                    return None;
                }
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read as usize]);
                if body.len() > MAX_BODY {
                    return None;
                }
            }
            Some((status, body))
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            (self.api.close)(self.connection);
            (self.api.close)(self.session);
        }
    }
}

/// Percent-encodes a query-string value (RFC 3986 unreserved characters pass through).
pub fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
