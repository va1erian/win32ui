//! Resolving OpenGL entry points for `glow`.

use core::ffi::c_void;
use std::ffi::CString;
use std::ptr;

use windows::Win32::Graphics::OpenGL::wglGetProcAddress;
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows::core::{PCSTR, s};

/// Resolves the OpenGL function called `name`, or null when it does not exist.
///
/// `wglGetProcAddress` only knows the functions a current context advertises,
/// and returns null for the OpenGL 1.1 core; those live in `opengl32.dll`, so
/// the loader falls back to that module's export table. A current context is
/// required for the first lookup, which the caller holds.
pub(crate) fn load(name: &str) -> *const c_void {
    let Ok(name) = CString::new(name) else {
        return ptr::null();
    };
    let pcstr = PCSTR(name.as_ptr() as *const u8);
    // SAFETY: `pcstr` is a NUL-terminated ASCII string valid for the call, and
    // the caller has a current GL context.
    if let Some(proc) = unsafe { wglGetProcAddress(pcstr) } {
        return proc as *const c_void;
    }
    // SAFETY: `opengl32.dll` is loaded (this crate links `wglGetProcAddress`
    // from it), and `pcstr` is valid for the call.
    let Ok(hmodule) = (unsafe { GetModuleHandleA(s!("opengl32.dll")) }) else {
        return ptr::null();
    };
    // SAFETY: `hmodule` is the live `opengl32.dll` and `pcstr` names an export.
    match unsafe { GetProcAddress(hmodule, pcstr) } {
        Some(proc) => proc as *const c_void,
        None => ptr::null(),
    }
}
