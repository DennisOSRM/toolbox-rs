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
    pub unsafe fn get_mut(&self, index: usize) -> &mut T {
        unsafe { &mut *self.slice[index].get() }
    }

    /// Returns a shared reference to the element at the given index.
    ///
    /// This function is safe to call because it returns an immutable reference,
    /// which can be shared between multiple threads. However, it's important to note
    /// that this safety relies on the exclusive access guarantee provided by the
    /// mutable reference passed to `UnsafeSlice::new()`.
    ///
    /// # Arguments
    ///
    /// * `index` - Position of the element to access
    ///
    /// # Returns
    ///
    /// Reference to the element at `index`
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds
    ///
    /// # Examples
    ///
    /// ```
    /// # use toolbox_rs::unsafe_slice::UnsafeSlice;
    /// let mut data = vec![1, 2, 3];
    /// let slice = UnsafeSlice::new(&mut data);
    /// assert_eq!(*slice.get(0), 1);
    /// ```
    pub fn get(&self, index: usize) -> &T {
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
        assert_eq!(*slice.get(0), 0);
        assert_eq!(*slice.get(1), 1);
        assert_eq!(*slice.get(2), 23);
        assert_eq!(*slice.get(3), 83);
        assert_eq!(*slice.get(4), 38);
        assert_eq!(*slice.get(5), 3);
        assert_eq!(*slice.get(6), 8947);
        assert_eq!(*slice.get(7), 2762);
    }

    #[test]
    fn test_get_mut() {
        let mut data = vec![1, 2, 3, 4];
        let slice = UnsafeSlice::new(&mut data);

        // SAFETY: We're only accessing each index once
        // and not sharing the slice between threads
        unsafe {
            *slice.get_mut(0) = 10;
            *slice.get_mut(2) = 30;
        }

        assert_eq!(*slice.get(0), 10);
        assert_eq!(*slice.get(1), 2);
        assert_eq!(*slice.get(2), 30);
        assert_eq!(*slice.get(3), 4);

        // Verify we can read the modified values
        assert_eq!(data[0], 10);
        assert_eq!(data[2], 30);
    }
}
