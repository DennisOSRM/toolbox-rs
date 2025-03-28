//! Implementation of Andrew's monotone chain convex hull algorithm
//! The runtime is $O(n\log n)$ by sorting the points lexicographically by
//! their lon/lat coordinates, and by subsequently constructing upper and
//! lower hulls.
//!
//! Note that the sorting order is lon/lat to make sure the x coordinate has
//! higher precedence than the y coordinate -- an invariant of the algorithm.

use crate::geometry::{FPCoordinate, is_clock_wise_turn};

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
            && !is_clock_wise_turn(&stack[stack.len() - 2], &stack[stack.len() - 1], p)
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
            && !is_clock_wise_turn(&stack[stack.len() - 2], &stack[stack.len() - 1], p)
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
    use crate::{convex_hull::monotone_chain, geometry::FPCoordinate};

    #[test]
    fn grid() {
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

    #[test]
    fn handle_overflow() {
        let coordinates = vec![
            FPCoordinate::new_from_lat_lon(33.424732, -114.905286),
            FPCoordinate::new_from_lat_lon(33.412828, -114.981799),
            FPCoordinate::new_from_lat_lon(33.402066, -114.978244),
            FPCoordinate::new_from_lat_lon(33.406161, -114.974526),
            FPCoordinate::new_from_lat_lon(33.393332, -115.000801),
            FPCoordinate::new_from_lat_lon(33.393065, -114.981161),
            FPCoordinate::new_from_lat_lon(33.383992, -114.994943),
            FPCoordinate::new_from_lat_lon(33.415325, -114.933815),
            FPCoordinate::new_from_lat_lon(33.413086, -114.941854),
            FPCoordinate::new_from_lat_lon(33.376757, -114.990162),
            FPCoordinate::new_from_lat_lon(33.373506, -114.970202),
            FPCoordinate::new_from_lat_lon(33.439025, -114.898966),
            FPCoordinate::new_from_lat_lon(33.432417, -114.932620),
            FPCoordinate::new_from_lat_lon(33.438574, -114.913486),
            FPCoordinate::new_from_lat_lon(33.415171, -114.945400),
            FPCoordinate::new_from_lat_lon(33.429861, -114.935991),
            FPCoordinate::new_from_lat_lon(33.413931, -114.968911),
            FPCoordinate::new_from_lat_lon(33.413785, -115.000715),
            FPCoordinate::new_from_lat_lon(33.395238, -114.987989),
            FPCoordinate::new_from_lat_lon(33.390153, -114.990825),
            FPCoordinate::new_from_lat_lon(33.388738, -114.979194),
            FPCoordinate::new_from_lat_lon(33.387090, -114.975945),
            FPCoordinate::new_from_lat_lon(33.382099, -114.974277),
            FPCoordinate::new_from_lat_lon(33.375377, -114.984210),
            FPCoordinate::new_from_lat_lon(33.430011, -114.903102),
            FPCoordinate::new_from_lat_lon(33.424118, -114.909812),
            FPCoordinate::new_from_lat_lon(33.412820, -114.943641),
            FPCoordinate::new_from_lat_lon(33.430089, -114.903063),
            FPCoordinate::new_from_lat_lon(33.359699, -114.945064),
            FPCoordinate::new_from_lat_lon(33.413760, -115.000801),
            FPCoordinate::new_from_lat_lon(33.434750, -114.929788),
            FPCoordinate::new_from_lat_lon(33.412851, -114.948184),
            FPCoordinate::new_from_lat_lon(33.395008, -114.991292),
            FPCoordinate::new_from_lat_lon(33.385784, -114.979111),
            FPCoordinate::new_from_lat_lon(33.406637, -115.000801),
            FPCoordinate::new_from_lat_lon(33.440700, -114.920131),
        ];

        let expected = vec![
            FPCoordinate::new(33393332, -115000801),
            FPCoordinate::new(33383992, -114994943),
            FPCoordinate::new(33376756, -114990162),
            FPCoordinate::new(33359699, -114945064),
            FPCoordinate::new(33424732, -114905286),
            FPCoordinate::new(33439025, -114898966),
            FPCoordinate::new(33440700, -114920131),
            FPCoordinate::new(33413760, -115000801),
        ];
        let convex_hull = monotone_chain(&coordinates);
        assert_eq!(expected, convex_hull);
    }

    #[test]
    fn tiny_instance() {
        let coordinates = vec![
            FPCoordinate::new_from_lat_lon(33.424732, -114.905286),
            FPCoordinate::new_from_lat_lon(33.412828, -114.981799),
            FPCoordinate::new_from_lat_lon(33.440700, -114.920131),
        ];

        let expected = vec![
            FPCoordinate::new(33424732, -114905286),
            FPCoordinate::new(33412827, -114981799),
            FPCoordinate::new(33440700, -114920131),
        ];
        let convex_hull = monotone_chain(&coordinates);
        assert_eq!(expected, convex_hull);
    }
}
