use std::cell::UnsafeCell;

/// This struct serves as a wrapper around unsafe accesses to a vector of
/// elements.
///
/// This can be used if (and only if) we know (and the compiler doesn't) that
/// threads do not access the same index during the concurrent processing.
///
/// DO NOT REUSE IN YOUR PROJECTS!
///
#[derive(Copy, Clone)]
pub struct UnsafeSlice<'a, T> {
    slice: &'a [UnsafeCell<T>],
}
unsafe impl<'a, T: Send + Sync> Send for UnsafeSlice<'a, T> {}
unsafe impl<'a, T: Send + Sync> Sync for UnsafeSlice<'a, T> {}

impl<'a, T> UnsafeSlice<'a, T> {
    pub fn new(slice: &'a mut [T]) -> Self {
        let ptr = slice as *mut [T] as *const [UnsafeCell<T>];
        Self {
            slice: unsafe { &*ptr },
        }
    }

    ///  # Safety
    ///  Two threads concurrently writing to the same location will cause UB!!
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get(&self, index: usize) -> &mut T {
        &mut *self.slice[index].get()
    }
}
