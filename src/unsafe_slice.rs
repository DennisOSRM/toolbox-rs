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
unsafe impl<T: Send + Sync> Send for UnsafeSlice<'_, T> {}
unsafe impl<T: Send + Sync> Sync for UnsafeSlice<'_, T> {}

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
        unsafe { &mut *self.slice[index].get() }
    }
}

#[cfg(test)]
mod tests {
    use super::UnsafeSlice;

    #[test]
    fn instantiate() {
        let mut data = vec![0, 1, 23, 83, 38, 3, 8947, 2762];
        let slice = UnsafeSlice::new(&mut data);
        assert_eq!(unsafe { *slice.get(0) }, 0);
        assert_eq!(unsafe { *slice.get(1) }, 1);
        assert_eq!(unsafe { *slice.get(2) }, 23);
        assert_eq!(unsafe { *slice.get(3) }, 83);
        assert_eq!(unsafe { *slice.get(4) }, 38);
        assert_eq!(unsafe { *slice.get(5) }, 3);
        assert_eq!(unsafe { *slice.get(6) }, 8947);
        assert_eq!(unsafe { *slice.get(7) }, 2762);
    }
}
