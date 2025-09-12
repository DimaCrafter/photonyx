use std::{ops::Deref, sync::LazyLock};


pub trait LazyInit {
    fn init () -> Self;
}

/// Thread-safe storage for lazy initialized values
///
/// Example:
/// ```
/// struct MyStatic {
///     pub a: u32
/// }
///
/// impl LazyInit for MyStatic {
///     fn init () -> Self {
///         MyStatic { a: 42 }
///     }
/// }
///
/// static MY_STATIC: AppStatic<MyStatic> = AppStatic::new();
/// fn test_static () {
///     println!("a = {}", MY_STATIC.a);
/// }
/// ```
pub struct AppStatic<T> {
    lock: LazyLock<T>
}

impl<T: LazyInit> AppStatic<T> {
    #[inline]
    pub const fn new () -> Self {
        AppStatic { lock: LazyLock::new(T::init) }
    }
}

impl<T> Deref for AppStatic<T> {
    type Target = T;

    #[inline]
    fn deref (&self) -> &T {
        return LazyLock::force(&self.lock);
    }
}
