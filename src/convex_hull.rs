//! Implementation of Andrew's monotone chain convex hull algorithm
//! The runtime is $O(n\log n)$ by sorting the points lexicographically by
//! their lon/lat coordinates, and by subsequently constructing upper and
//! lower hulls.
//!
//! Note that the sorting order is lon/lat to make sure the x coordinate has
//! higher precedence than the y coordinate -- an invariant of the algorithm.

use crate::geometry::primitives::{cross_product, FPCoordinate};

pub fn monotone_chain(input_coordinates: &[FPCoordinate]) -> Vec<FPCoordinate> {
    let n = input_coordinates.len();
    if n <= 3 {
        return input_coordinates.into();
    }

    // TODO: Implement heuristic by Akl-Toussaint to quickly exclude points

    let mut coordinates: Vec<_> = input_coordinates.into();
    coordinates.sort_unstable_by_key(|a| (a.lon, a.lat));

    // assemble the hull
    let mut stack = Vec::new();
    coordinates.iter().for_each(|p| {
        while stack.len() >= 2
            && cross_product(&stack[stack.len() - 2], &stack[stack.len() - 1], p) <= 0
        {
            stack.pop();
        }
        stack.push(*p);
    });
    // remove the last element since they are repeated in the beginning of the other half
    stack.pop();

    // upper hull
    let lower_stack_len = stack.len();
    coordinates.iter().rev().for_each(|p| {
        while stack.len() >= (2 + lower_stack_len)
            && cross_product(&stack[stack.len() - 2], &stack[stack.len() - 1], p) <= 0
        {
            stack.pop();
        }
        stack.push(*p);
    });

    // remove the last element since they are repeated in the beginning of the other half
    stack.pop();
    stack
}

#[cfg(test)]
mod tests {
    use crate::{convex_hull::monotone_chain, geometry::primitives::FPCoordinate};

    #[test]
    pub fn grid() {
        let mut coordinates: Vec<FPCoordinate> = Vec::new();
        for i in 0..100 {
            coordinates.push(FPCoordinate::new(i / 10, i % 10));
        }

        let expected = vec![
            FPCoordinate::new(0, 0),
            FPCoordinate::new(0, 9),
            FPCoordinate::new(9, 9),
            FPCoordinate::new(9, 0),
        ];
        let result = monotone_chain(&coordinates);
        assert_eq!(expected, result);
    }
}
