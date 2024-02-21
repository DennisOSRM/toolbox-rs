use std::collections::VecDeque;
use std::fmt::Debug;
use std::{cell::RefCell, rc::Rc};

#[derive(Debug, Clone)]
pub struct HuffmanNode<T> {
    character: Option<T>,
    frequency: i32,
    left: Option<HuffmanNodeRef<T>>,
    right: Option<HuffmanNodeRef<T>>,
}

type HuffmanNodeRef<T> = Rc<RefCell<HuffmanNode<T>>>;

fn min_node<T>(
    q1: &mut VecDeque<HuffmanNode<T>>,
    q2: &mut VecDeque<HuffmanNode<T>>,
) -> HuffmanNode<T> {
    if q1.is_empty() {
        return q2.pop_front().unwrap();
    }

    if q2.is_empty() {
        return q1.pop_front().unwrap();
    }

    if q1.front().unwrap().frequency < q2.front().unwrap().frequency {
        return q1.pop_front().unwrap();
    }
    q2.pop_front().unwrap()
}

pub fn generate_huffman_code_from_sorted<T: Copy + Debug>(
    v: &[(T, i32)],
) -> Vec<(T, std::string::String)> {
    if v.is_empty() {
        return Vec::new();
    }

    let mut q1 = VecDeque::new();
    let mut q2 = VecDeque::new();

    for (t, f) in v {
        q1.push_back(HuffmanNode::<T> {
            character: Some(*t),
            frequency: *f,
            left: None,
            right: None,
        })
    }

    while !q1.is_empty() || q2.len() > 1 {
        let left = min_node(&mut q1, &mut q2);
        let right = min_node(&mut q1, &mut q2);
        let node = HuffmanNode::<T> {
            character: None, // None signifies that this is an interior node
            frequency: left.frequency + right.frequency,
            left: Some(Rc::new(RefCell::new(left))),
            right: Some(Rc::new(RefCell::new(right))),
        };
        q2.push_back(node);
    }

    let root = Rc::new(RefCell::new(q2.pop_front().unwrap()));

    // generate code book
    let mut code_book = Vec::new();
    let mut stack = Vec::new();
    stack.push((root.clone(), String::new()));

    while let Some((current, prefix)) = stack.pop() {
        if let Some(character) = current.borrow().character {
            code_book.push((character, prefix));
        } else {
            if let Some(left) = current.borrow().left.clone() {
                stack.push((left, prefix.clone() + "0"));
            }
            if let Some(right) = current.borrow().right.clone() {
                stack.push((right, prefix.clone() + "1"));
            }
        };
    }

    code_book
}

#[cfg(test)]
mod tests {
    use super::generate_huffman_code_from_sorted;

    #[test]
    fn construction() {
        let v = [
            ('a', 5),
            ('b', 9),
            ('c', 12),
            ('d', 13),
            ('e', 16),
            ('f', 45),
        ];
        let code_book = generate_huffman_code_from_sorted(&v);
        let expected = [
            ('f', "0"),
            ('c', "100"),
            ('d', "101"),
            ('a', "1100"),
            ('b', "1101"),
            ('e', "111"),
        ];

        let matching = code_book
            .iter()
            .rev()
            .zip(&expected)
            .filter(|&(a, b)| a.0 == b.0 && a.1 == b.1)
            .count();
        assert!(matching == code_book.len());
        assert!(matching == expected.len());
    }
}
