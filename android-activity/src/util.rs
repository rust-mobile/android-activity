use std::{ffi::CStr, os::raw::c_char, ptr};

pub fn try_get_path_from_ptr(path: *const c_char) -> Option<std::path::PathBuf> {
    if path == ptr::null() {
        return None;
    }
    let cstr = unsafe {
        let cstr_slice = CStr::from_ptr(path.cast());
        cstr_slice.to_str().ok()?
    };
    if cstr.len() == 0 {
        return None;
    }
    Some(std::path::PathBuf::from(cstr))
}
