use std::{cmp::PartialOrd, fmt::Debug};

#[derive(Debug)]
struct Node<T> {
    next: Option<Box<Node<T>>>,
    elem: T,
}

/// A singly-linked list implementation with sorting capabilities.
///
/// # Type Parameters
///
/// * `T` - The element type, which must be `Copy`, `Debug`, and `PartialOrd`
///
/// # Examples
///
/// ```
/// use toolbox_rs::single_linked_list::SingleLinkedList;
///
/// let mut list = SingleLinkedList::new();
/// list.push_front(1);
/// list.push_front(2);
/// assert_eq!(list.pop_front(), Some(2));
/// ```
pub struct SingleLinkedList<T> {
    head: Option<Box<Node<T>>>,
}

impl<T: Copy + Debug + PartialOrd> SingleLinkedList<T> {
    /// Creates a new empty linked list.
    ///
    /// # Examples
    /// ```
    /// use toolbox_rs::single_linked_list::SingleLinkedList;
    /// let list: SingleLinkedList<i32> = SingleLinkedList::new();
    /// assert!(list.is_empty());
    /// ```
    pub fn new() -> Self {
        Self { head: None }
    }

    /// Adds an element to the front of the list.
    ///
    /// # Arguments
    /// * `elem` - The element to add
    ///
    /// # Examples
    /// ```
    /// use toolbox_rs::single_linked_list::SingleLinkedList;
    /// let mut list = SingleLinkedList::new();
    /// list.push_front(1);
    /// assert_eq!(list.peek_front(), Some(&1));
    /// ```
    pub fn push_front(&mut self, elem: T) {
        let new_node = Box::new(Node {
            next: self.head.take(),
            elem,
        });
        self.head = Some(new_node);
    }

    /// Removes and returns the first element of the list.
    ///
    /// # Returns
    /// * `Some(T)` - The first element if the list is not empty
    /// * `None` - If the list is empty
    ///
    /// # Examples
    /// ```
    /// use toolbox_rs::single_linked_list::SingleLinkedList;
    /// let mut list = SingleLinkedList::new();
    /// list.push_front(1);
    /// assert_eq!(list.pop_front(), Some(1));
    /// assert_eq!(list.pop_front(), None);
    /// ```
    pub fn pop_front(&mut self) -> Option<T> {
        self.head.take().map(|node| {
            self.head = node.next;
            node.elem
        })
    }

    /// Returns a reference to the first element without removing it.
    ///
    /// # Returns
    /// * `Some(&T)` - Reference to the first element if the list is not empty
    /// * `None` - If the list is empty
    pub fn peek_front(&self) -> Option<&T> {
        self.head.as_ref().map(|node| &node.elem)
    }

    /// Returns a mutable reference to the first element without removing it.
    ///
    /// # Returns
    /// * `Some(&mut T)` - Mutable reference to the first element if the list is not empty
    /// * `None` - If the list is empty
    pub fn peek_front_mut(&mut self) -> Option<&mut T> {
        self.head.as_mut().map(|node| &mut node.elem)
    }

    /// Checks if the list is empty.
    ///
    /// # Returns
    /// * `true` - If the list contains no elements
    /// * `false` - If the list contains at least one element
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    /// Checks if the list is sorted in ascending order.
    ///
    /// # Returns
    /// * `true` - If the list is sorted or has fewer than 2 elements
    /// * `false` - If the list is not sorted
    pub fn is_sorted(&self) -> bool {
        let mut current = &self.head;
        while let Some(node) = current {
            if let Some(next_node) = &node.next {
                if node.elem > next_node.elem {
                    return false;
                }
            }
            current = &node.next;
        }
        true
    }

    /// Inserts an element into the list maintaining sorted order.
    ///
    /// # Arguments
    /// * `elem` - The element to insert
    ///
    /// # Note
    /// Assumes the list is already sorted. If the list is not sorted,
    /// the resulting order is undefined.
    pub fn insert_sorted(&mut self, elem: T) {
        let mut current = &mut self.head;
        while let Some(node) = current {
            let next_is_smaller = match &node.next {
                Some(next) => next.elem < elem,
                None => false,
            };

            if next_is_smaller {
                current = &mut node.next;
            } else {
                let new_node = Box::new(Node {
                    next: node.next.take(),
                    elem,
                });
                node.next = Some(new_node);
                return;
            }
        }
    }

    /// Removes all elements from the list.
    ///
    /// # Examples
    /// ```
    /// use toolbox_rs::single_linked_list::SingleLinkedList;
    /// let mut list = SingleLinkedList::new();
    /// list.push_front(1);
    /// list.clear();
    /// assert!(list.is_empty());
    /// ```
    pub fn clear(&mut self) {
        self.head = None;
    }
}

impl<T: Copy + Debug + PartialOrd> Default for SingleLinkedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn creation_push_peek_pop() {
        let mut list = super::SingleLinkedList::new();
        assert!(list.is_empty());
        list.push_front(1);
        list.push_front(2);
        list.push_front(3);
        assert!(!list.is_empty());
        assert_eq!(list.peek_front(), Some(&3));
        assert_eq!(list.peek_front_mut(), Some(&mut 3));
        assert_eq!(list.pop_front(), Some(3));
        assert_eq!(list.pop_front(), Some(2));
        assert_eq!(list.pop_front(), Some(1));
        assert_eq!(list.pop_front(), None);
        assert!(list.is_empty());
    }

    #[test]
    fn find_not_less() {
        let mut list = super::SingleLinkedList::new();
        assert!(list.is_empty());
        assert!(list.is_sorted());
        list.push_front(8);
        list.push_front(5);
        list.push_front(1);
        assert!(list.is_sorted());

        list.insert_sorted(3);
        list.insert_sorted(2);
        assert!(list.is_sorted());
        list.insert_sorted(6);
        list.insert_sorted(4);
        list.insert_sorted(7);
        list.insert_sorted(9);
    }

    #[test]
    fn unsorted() {
        let mut list = super::SingleLinkedList::default();
        assert!(list.is_empty());
        assert!(list.is_sorted());
        list.push_front(5);
        list.push_front(8);
        list.push_front(1);
        assert!(!list.is_sorted());
    }

    #[test]
    fn clear_list() {
        let mut list = super::SingleLinkedList::new();

        // Clear empty list
        list.clear();
        assert!(list.is_empty());

        // Add elements and clear
        list.push_front(1);
        list.push_front(2);
        list.push_front(3);
        assert!(!list.is_empty());
        assert_eq!(list.peek_front(), Some(&3));

        list.clear();
        assert!(list.is_empty());
        assert_eq!(list.peek_front(), None);

        // Verify operations work after clearing
        list.push_front(4);
        assert!(!list.is_empty());
        assert_eq!(list.peek_front(), Some(&4));
    }
}
