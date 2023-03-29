use std::{ffi::CStr, os::raw::c_char};

use log::Level;

pub fn try_get_path_from_ptr(path: *const c_char) -> Option<std::path::PathBuf> {
    if path.is_null() {
        return None;
    }
    let cstr = unsafe {
        let cstr_slice = CStr::from_ptr(path.cast());
        cstr_slice.to_str().ok()?
    };
    if cstr.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(cstr))
}

pub(crate) fn android_log(level: Level, tag: &CStr, msg: &CStr) {
    let prio = match level {
        Level::Error => ndk_sys::android_LogPriority::ANDROID_LOG_ERROR,
        Level::Warn => ndk_sys::android_LogPriority::ANDROID_LOG_WARN,
        Level::Info => ndk_sys::android_LogPriority::ANDROID_LOG_INFO,
        Level::Debug => ndk_sys::android_LogPriority::ANDROID_LOG_DEBUG,
        Level::Trace => ndk_sys::android_LogPriority::ANDROID_LOG_VERBOSE,
    };
    unsafe {
        ndk_sys::__android_log_write(prio.0 as libc::c_int, tag.as_ptr(), msg.as_ptr());
    }
}
