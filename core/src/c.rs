use std::{alloc::{self, Layout}, ffi::c_char, ptr::slice_from_raw_parts};

#[no_mangle]
pub unsafe extern "C" fn ra_alloc (size: usize, align: usize) -> *mut u8 {
    return alloc::alloc(Layout::from_size_align_unchecked(size, align));
}

#[no_mangle]
pub unsafe extern "C" fn ra_realloc (ptr: *mut u8, old_size: usize, new_size: usize, align: usize) -> *mut u8 {
    return alloc::realloc(ptr, Layout::from_size_align_unchecked(old_size, align), new_size);
}

#[no_mangle]
pub unsafe extern "C" fn ra_dealloc (ptr: *mut u8, size: usize, align: usize) {
    return alloc::dealloc(ptr, Layout::from_size_align_unchecked(size, align));
}

#[allow(non_camel_case_types)]
pub type c_str = *const c_char;

#[inline(always)]
pub fn c_init<T> (ctor: fn () -> T) -> *mut T {
	return Box::into_raw(Box::new((ctor)()));
}

#[inline(always)]
pub unsafe fn c_unwrap<T> (ptr: *mut T) -> T {
	return *Box::from_raw(ptr);
}

#[inline(always)]
pub unsafe fn c_deinit<T> (ptr: *mut T) {
	let _ = c_unwrap(ptr);
}

pub fn c_strlen (raw: c_str) -> usize {
	if raw.is_null() {
        return 0;
    }

    let mut len = 0;
    unsafe {
        let mut ptr = raw;
        while *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
    }

    return len;
}

pub unsafe fn c_string (ptr: c_str) -> String {
	let raw = &*slice_from_raw_parts(ptr as *const u8, c_strlen(ptr));
	return String::from_utf8_lossy(raw).into_owned();
}