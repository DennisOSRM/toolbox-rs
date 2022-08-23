/// Simplified implementation of a linked list that is suitable for a textbook
/// implementation of an LRU cache. The code below implements three important
/// functions upon which the cache is built:
///
/// 1) push_front(): insert an element to the front of the list
/// 2) pop_back(): remove the back element if it exists, and
/// 3) move_to_front(): move an existing element to the front of the list
///
/// The implementation is modelled after the excellent writeup on implementing
/// linked lists called "Learn Rust With Entirely Too Many Linked Lists" which
/// can be found at https://rust-unofficial.github.io/too-many-lists/index.html
///
/// Since linked lists use pointers and are self-referential, there's a ton of
/// unsafe code in the implementation. Please be mindful, when changing the code.
///
/// Once the standard library contains a stable implementation of linked lists
/// with cursors this code could be removed. We are not there yet as of writing.
use std::marker::PhantomData;
use std::ptr::NonNull;

pub struct LinkedList<T> {
    front: Link<T>,
    back: Link<T>,
    len: usize,
    _ghost: PhantomData<T>,
}
pub type ListCursor<T> = NonNull<Node<T>>;
type Link<T> = Option<ListCursor<T>>;

pub struct Node<T> {
    next: Link<T>,
    prev: Link<T>,
    elem: T,
}

impl<T> LinkedList<T> {
    pub fn new() -> Self {
        Self {
            front: None,
            back: None,
            len: 0,
            _ghost: PhantomData,
        }
    }

    pub fn push_front(&mut self, elem: T) -> ListCursor<T> {
        // SAFETY: it's a linked-list, what do you want?
        unsafe {
            let new = NonNull::new_unchecked(Box::into_raw(Box::new(Node {
                next: None,
                prev: None,
                elem,
            })));
            if let Some(old) = self.front {
                // Put the new front before the old one
                (*old.as_ptr()).next = Some(new);
                (*new.as_ptr()).prev = Some(old);
            } else {
                // If there's no front, then we're the empty list and need
                // to set the back too.
                self.back = Some(new);
            }
            // These things always happen!
            self.front = Some(new);
            self.len += 1;

            new
        }
    }

    pub fn move_to_front(&mut self, b: &ListCursor<T>) {
        if self.is_empty() {
            return;
        }

        if let Some(front) = self.front {
            // Is node B already in front?
            if front == *b {
                return;
            }
        }

        // SAFETY: it's a linked-list, what do you want?
        unsafe {
            // cut node b from list by short-cutting a<->c
            let a = (*b.as_ptr()).next;
            (*a.unwrap().as_ptr()).prev = (*b.as_ptr()).prev;
            if let Some(c) = (*b.as_ptr()).prev {
                (*c.as_ptr()).next = (*b.as_ptr()).next;
            }

            // if the last element is moved to front, then update it with the next element in row
            if self.back.unwrap() == *b {
                debug_assert!((*b.as_ptr()).prev.is_none());
                self.back = a;
            }
        }

        // SAFETY: it's a linked-list, what do you want?
        unsafe {
            // move now-floating node b to the front of the linked list
            let x = self.front;

            (*b.as_ptr()).prev = x;
            (*b.as_ptr()).next = None;
            (*x.unwrap().as_ptr()).next = Some(*b);
            self.front = Some(*b);
        }
    }

    pub fn pop_back(&mut self) -> Option<T> {
        unsafe {
            // Only have to do stuff if there is a back node to pop.
            self.back.map(|node| {
                // Bring the Box front to life so we can move out its value and
                // Drop it (Box continues to magically understand this for us).
                let boxed_node = Box::from_raw(node.as_ptr());
                let result = boxed_node.elem;

                // Make the next node into the new back.
                self.back = boxed_node.next;
                if let Some(new) = self.back {
                    // Cleanup its reference to the removed node
                    (*new.as_ptr()).prev = None;
                } else {
                    // If the back is now null, then this list is now empty!
                    self.front = None;
                }

                self.len -= 1;
                result
                // Box gets implicitly freed here, knows there is no T.
            })
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        // Oh look it's drop again
        while self.pop_back().is_some() {}
    }

    pub fn get_front(&self) -> &T {
        // TODO: decide whether this returns a reference or a copy
        unsafe { &self.front.unwrap().as_ref().elem }
    }
}

impl<T> Drop for LinkedList<T> {
    fn drop(&mut self) {
        self.clear()
    }
}

impl<T> Default for LinkedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use super::LinkedList;

    #[test]
    fn test_basic_front() {
        let mut list = LinkedList::new();

        assert_eq!(list.len(), 0);
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);

        list.push_front(10);
        assert_eq!(list.len(), 1);
        assert_eq!(list.pop_back(), Some(10));
        assert_eq!(list.len(), 0);
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);

        list.push_front(10);
        assert_eq!(list.len(), 1);
        list.push_front(20);
        assert_eq!(list.len(), 2);
        list.push_front(30);
        assert_eq!(list.len(), 3);
        assert_eq!(list.pop_back(), Some(10));
        assert_eq!(list.len(), 2);
        list.push_front(40);
        assert_eq!(list.len(), 3);
        assert_eq!(list.pop_back(), Some(20));
        assert_eq!(list.len(), 2);
        assert_eq!(list.pop_back(), Some(30));
        assert_eq!(list.len(), 1);
        assert_eq!(list.pop_back(), Some(40));
        assert_eq!(list.len(), 0);
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn basic_move_to_front() {
        let mut list = LinkedList::new();

        assert_eq!(list.len(), 0);
        let first_inserted = list.push_front(1);
        list.move_to_front(&first_inserted);

        list.push_front(5);
        list.push_front(4);
        list.push_front(3);
        list.push_front(2);

        list.move_to_front(&first_inserted);

        list.push_front(0);

        let mut result = Vec::new();
        while let Some(element) = list.pop_back() {
            result.push(element);
        }

        assert_eq!(result, vec![5, 4, 3, 2, 1, 0]);
    }

    #[test]
    fn push_sort_move() {
        // test idea:
        // - nodes handles are stored in an array
        // - sort array by element
        // - run move_to_front on all elements in order of sorted array
        // - output should be sorted
        let mut list = LinkedList::new();
        let mut handles = Vec::new();
        assert_eq!(list.len(), 0);
        handles.push(list.push_front(1));
        handles.push(list.push_front(5));
        handles.push(list.push_front(2));
        handles.push(list.push_front(4));
        handles.push(list.push_front(3));

        handles.sort_by_key(|h| unsafe { h.as_ref().elem });

        handles.iter().for_each(|handle| {
            list.move_to_front(handle);
        });

        let mut result = Vec::new();
        while let Some(element) = list.pop_back() {
            result.push(element);
        }

        assert_eq!(result, vec![1, 2, 3, 4, 5]);
    }
}
