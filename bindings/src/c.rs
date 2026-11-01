use std::fmt::Debug;


#[repr(C)]
#[derive(Debug)]
/// Represents const fat pointer to `T`
pub struct Slice<T> {
	ptr: *const T,
	len: usize
}

impl<T> Slice<T> {
    pub const fn null () -> Slice<T> {
        Slice {
            ptr: std::ptr::null(),
            len: 0
        }
    }

    pub fn for_vec (value: &Vec<T>) -> Slice<T> {
        Slice {
            ptr: value.as_ptr(),
            len: value.len()
        }
    }

    fn as_native (&self) -> &[T] {
        return unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
    }
}

impl<S: AsRef<str>> From<S> for Slice<u8> {
    fn from (value: S) -> Self {
        Slice {
            ptr: value.as_ref().as_ptr(),
            len: value.as_ref().len()
        }
    }
}

impl<'a, T> IntoIterator for &'a Slice<T> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;

    fn into_iter (self) -> Self::IntoIter {
        return self.as_native().iter();
    }
}

impl Slice<u8> {
    pub unsafe fn as_str_unchecked(&self) -> &str {
        return unsafe { std::str::from_utf8_unchecked(self.as_native()) };
    }
}

#[inline]
pub fn disown<T: Sized> (value: T) -> *mut T {
    return Box::into_raw(Box::new(value));
}

#[inline]
pub unsafe fn own<T> (ptr: *mut T) -> Box<T> {
    return Box::from_raw(ptr);
}
