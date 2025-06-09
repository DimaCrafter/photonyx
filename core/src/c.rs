use std::{alloc::{self, Layout}, ffi::{c_char, CString}, ptr::slice_from_raw_parts};

#[no_mangle]
pub unsafe extern "C" fn rs_alloc (size: usize, align: usize) -> *mut u8 {
    return alloc::alloc(Layout::from_size_align_unchecked(size, align));
}

#[no_mangle]
pub unsafe extern "C" fn rs_realloc (ptr: *mut u8, old_size: usize, new_size: usize, align: usize) -> *mut u8 {
    return alloc::realloc(ptr, Layout::from_size_align_unchecked(old_size, align), new_size);
}

#[no_mangle]
pub unsafe extern "C" fn rs_dealloc (ptr: *mut u8, size: usize, align: usize) {
    return alloc::dealloc(ptr, Layout::from_size_align_unchecked(size, align));
}

#[no_mangle]
pub unsafe extern "C" fn str_drop (ptr: c_str_mut) {
    return c_deinit_str(ptr);
}

#[allow(non_camel_case_types)]
pub type c_str = *const c_char;
#[allow(non_camel_case_types)]
pub type c_str_mut = *mut c_char;

#[inline(always)]
pub fn c_init<T, C: Fn () -> T> (ctor: C) -> *mut T {
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

#[inline(always)]
pub fn c_init_str<T: Into<Vec<u8>>> (value: T) -> c_str {
	return CString::new(value).unwrap().into_raw();
}

#[inline(always)]
pub unsafe fn c_deinit_str (ptr: c_str_mut) {
    let _ = CString::from_raw(ptr);
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

#[repr(C)]
pub struct Slice<T> {
	ptr: *const T,
	len: usize
}

impl<T> Slice<T> {
    pub fn for_vec(value: &Vec<T>) -> Slice<T> {
        Slice {
            ptr: value.as_ptr(),
            len: value.len()
        }
    }
}
