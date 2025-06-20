use std::fmt::Debug;


#[repr(C)]
#[derive(Debug)]
/// Represents const fat pointer to `T`
pub struct Slice<T> {
	ptr: *const T,
	len: usize
}

impl<T> Slice<T> {
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
