// Computes the Minimum Spanning Tree (MST) of a complete graph without copying all edges.
// Uses Prim's algorithm for efficiency.
//
// Time complexity: O(n^2 log n) worst case for a complete graph with n nodes.
// In practice, it's closer to O(n^2) because:
// 1. Each node is processed exactly once (O(n))
// 2. For each node, we scan all others (O(n))
// 3. Heap operations (push/pop) are O(log h) where h is the heap size
// 4. The heap may contain multiple entries for the same node, but most are skipped
// Space complexity: O(n) for the auxiliary arrays and O(n) for the output edges.
// This is optimal for dense graphs and avoids O(n^2) edge copying.

use crate::complete_graph::CompleteGraph;
use crate::edge::SimpleEdge;
use std::collections::BinaryHeap;

/// Computes the Minimum Spanning Tree (MST) of a complete graph using Prim's algorithm.
///
/// # Complexity
/// - Time: O(n^2 log n) worst case for n nodes, but often performs closer to O(n^2) in practice
///   because the binary heap operations are more efficient than their worst-case bounds
/// - Space: O(n) for auxiliary data, O(n) for output edges
///
/// # Arguments
/// * `graph` - Reference to a `CompleteGraph` with edge weights convertible to `u32`.
///
/// # Returns
/// Tuple of (total_cost, Vec of MST edges as `SimpleEdge`)
///
/// # Example
/// ```
/// use toolbox_rs::complete_graph::CompleteGraph;
/// use toolbox_rs::prim_complete_graph::mst_prim;
/// let mut g = CompleteGraph::new(3);
/// g[(0, 1)] = 2u32;
/// g[(1, 0)] = 2u32;
/// g[(0, 2)] = 1u32;
/// g[(2, 0)] = 1u32;
/// g[(1, 2)] = 3u32;
/// g[(2, 1)] = 3u32;
/// let (cost, mst) = mst_prim(&g);
/// assert_eq!(cost, 3); // 1 + 2
/// assert_eq!(mst.len(), 2);
/// ```
pub fn mst_prim<T>(graph: &CompleteGraph<T>) -> (u32, Vec<SimpleEdge>)
where
    T: Copy + Into<u32> + Default + PartialEq + std::fmt::Debug,
{
    let n = graph.num_nodes();
    if n == 0 {
        return (0, Vec::new());
    }
    let mut in_mst = vec![false; n];
    let mut min_edge = vec![u32::MAX; n];
    let mut parent = vec![None; n];
    let mut heap = BinaryHeap::new();
    min_edge[0] = 0;
    heap.push((std::cmp::Reverse(0u32), 0));
    let mut total_cost = 0;
    let mut mst_edges = Vec::with_capacity(n - 1);
    while let Some((std::cmp::Reverse(cost), u)) = heap.pop() {
        if in_mst[u] {
            continue;
        }
        in_mst[u] = true;
        total_cost += cost;
        if let Some(p) = parent[u] {
            mst_edges.push(SimpleEdge::new(p, u, cost));
        }
        for v in 0..n {
            if !in_mst[v] {
                let weight = graph[(u, v)].into();
                if weight < min_edge[v] {
                    min_edge[v] = weight;
                    parent[v] = Some(u);
                    heap.push((std::cmp::Reverse(weight), v));
                }
            }
        }
    }
    (total_cost, mst_edges)
}

/// Computes only the total cost of the Minimum Spanning Tree (MST) of a complete graph using Prim's algorithm.
/// This is more efficient than `mst_prim` when only the cost is needed, as it avoids tracking parent nodes and
/// constructing the resulting edge list.
///
/// # Complexity
/// - Time: O(n^2 log n) worst case for n nodes, but often performs closer to O(n^2) in practice
///   because the binary heap operations are more efficient than their worst-case bounds
/// - Space: O(n) for auxiliary data
///
/// # Arguments
/// * `graph` - Reference to a `CompleteGraph` with edge weights convertible to `u32`.
///
/// # Returns
/// The total cost of the MST as a `u32`.
///
/// # Example
/// ```
/// use toolbox_rs::complete_graph::CompleteGraph;
/// use toolbox_rs::prim_complete_graph::mst_prim_cost_only;
/// let mut g = CompleteGraph::new(3);
/// g[(0, 1)] = 2u32;
/// g[(1, 0)] = 2u32;
/// g[(0, 2)] = 1u32;
/// g[(2, 0)] = 1u32;
/// g[(1, 2)] = 3u32;
/// g[(2, 1)] = 3u32;
/// let cost = mst_prim_cost_only(&g);
/// assert_eq!(cost, 3); // 1 + 2
/// ```
pub fn mst_prim_cost_only<T>(graph: &CompleteGraph<T>) -> u32
where
    T: Copy + Into<u32> + Default + PartialEq + std::fmt::Debug,
{
    let n = graph.num_nodes();
    if n == 0 {
        return 0;
    }
    let mut in_mst = vec![false; n];
    let mut min_edge = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    min_edge[0] = 0;
    heap.push((std::cmp::Reverse(0u32), 0));
    let mut total_cost = 0;
    while let Some((std::cmp::Reverse(cost), u)) = heap.pop() {
        if in_mst[u] {
            continue;
        }
        in_mst[u] = true;
        total_cost += cost;
        (0..n).filter(|v| !in_mst[*v]).for_each(|v| {
            let weight = graph[(u, v)].into();
            if weight < min_edge[v] {
                min_edge[v] = weight;
                heap.push((std::cmp::Reverse(weight), v));
            }
        });
    }
    total_cost
}

/// Computes the MST of a complete graph, treating the nodes in `fixed_subtour` as already included in the MST.
/// The MST is built over the remaining nodes, connecting them optimally to the fixed sub-tour.
/// Returns the total cost and the MST edges (excluding the sub-tour's own edges).
pub fn mst_prim_with_fixed_subtour<T>(
    graph: &CompleteGraph<T>,
    fixed_subtour: &[usize],
) -> (u32, Vec<SimpleEdge>)
where
    T: Copy + Into<u32> + Default + PartialEq + std::fmt::Debug,
{
    let n = graph.num_nodes();
    if n == 0 {
        return (0, Vec::new());
    }
    let mut in_mst = vec![false; n];
    for &u in fixed_subtour {
        in_mst[u] = true;
    }
    let mut min_edge = vec![u32::MAX; n];
    let mut parent = vec![None; n];
    let mut heap = BinaryHeap::new();
    // Initialize heap with all edges from fixed_subtour to the rest
    for &u in fixed_subtour {
        for v in 0..n {
            if !in_mst[v] {
                let weight = graph[(u, v)].into();
                if weight < min_edge[v] {
                    min_edge[v] = weight;
                    parent[v] = Some(u);
                    heap.push((std::cmp::Reverse(weight), v));
                }
            }
        }
    }
    let mut total_cost = 0;
    let mut mst_edges = Vec::with_capacity(n.saturating_sub(fixed_subtour.len()).saturating_sub(1));
    while let Some((std::cmp::Reverse(cost), u)) = heap.pop() {
        if in_mst[u] {
            continue;
        }
        in_mst[u] = true;
        total_cost += cost;
        if let Some(p) = parent[u] {
            mst_edges.push(SimpleEdge::new(p, u, cost));
        }
        for v in 0..n {
            if !in_mst[v] {
                let weight = graph[(u, v)].into();
                if weight < min_edge[v] {
                    min_edge[v] = weight;
                    parent[v] = Some(u);
                    heap.push((std::cmp::Reverse(weight), v));
                }
            }
        }
    }
    (total_cost, mst_edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complete_graph::CompleteGraph;

    #[test]
    fn test_mst_prim_empty_graph() {
        let g: CompleteGraph<u32> = CompleteGraph::new(0);
        let (cost, mst) = mst_prim(&g);
        assert_eq!(cost, 0);
        assert!(mst.is_empty());
    }

    #[test]
    fn test_mst_prim_single_node() {
        let g: CompleteGraph<u32> = CompleteGraph::new(1);
        let (cost, mst) = mst_prim(&g);
        assert_eq!(cost, 0);
        assert!(mst.is_empty());
    }

    #[test]
    fn test_mst_prim_small() {
        let mut g = CompleteGraph::new(4);
        g[(0, 1)] = 1u32;
        g[(1, 0)] = 1u32;
        g[(0, 2)] = 2u32;
        g[(2, 0)] = 2u32;
        g[(0, 3)] = 3u32;
        g[(3, 0)] = 3u32;
        g[(1, 2)] = 4u32;
        g[(2, 1)] = 4u32;
        g[(1, 3)] = 5u32;
        g[(3, 1)] = 5u32;
        g[(2, 3)] = 6u32;
        g[(3, 2)] = 6u32;
        let (cost, mst) = mst_prim(&g);
        assert_eq!(cost, 6); // 1 + 2 + 3
        assert_eq!(mst.len(), 3);
        let edge_set = mst
            .iter()
            .map(|e| (e.source.min(e.target), e.source.max(e.target)))
            .collect::<std::collections::HashSet<_>>();
        let expected = [(0, 1), (0, 2), (0, 3)];
        for &(a, b) in &expected {
            assert!(edge_set.contains(&(a, b)));
        }
    }

    #[test]
    fn test_mst_prim_duplicate_heap_entry() {
        // Create a graph where the heap will contain multiple entries for the same node
        // with different costs, and the node will already be in the MST when a duplicate is popped.
        // This will exercise the `if in_mst[u] { continue; }` branch.
        let mut g = CompleteGraph::new(3);
        g[(0, 1)] = 1u32;
        g[(1, 0)] = 1u32;
        g[(0, 2)] = 2u32;
        g[(2, 0)] = 2u32;
        g[(1, 2)] = 1u32;
        g[(2, 1)] = 1u32;
        // The MST is: 0-1 (1), 1-2 (1), total cost 2
        let (cost, mst) = mst_prim(&g);
        assert_eq!(cost, 2);
        assert_eq!(mst.len(), 2);
        let edge_set = mst
            .iter()
            .map(|e| (e.source.min(e.target), e.source.max(e.target)))
            .collect::<std::collections::HashSet<_>>();
        let expected = [(0, 1), (1, 2)];
        for &(a, b) in &expected {
            assert!(edge_set.contains(&(a, b)));
        }
    }

    #[test]
    fn test_mst_prim_and_cost_only_match() {
        // Create several test graphs of different sizes and compare results

        // Empty graph
        let g: CompleteGraph<u32> = CompleteGraph::new(0);
        assert_eq!(mst_prim(&g).0, mst_prim_cost_only(&g));

        // Single node
        let g: CompleteGraph<u32> = CompleteGraph::new(1);
        assert_eq!(mst_prim(&g).0, mst_prim_cost_only(&g));

        // Small complete graph
        let mut g = CompleteGraph::new(4);
        g[(0, 1)] = 1u32;
        g[(1, 0)] = 1u32;
        g[(0, 2)] = 2u32;
        g[(2, 0)] = 2u32;
        g[(0, 3)] = 3u32;
        g[(3, 0)] = 3u32;
        g[(1, 2)] = 4u32;
        g[(2, 1)] = 4u32;
        g[(1, 3)] = 5u32;
        g[(3, 1)] = 5u32;
        g[(2, 3)] = 6u32;
        g[(3, 2)] = 6u32;
        assert_eq!(mst_prim(&g).0, mst_prim_cost_only(&g));

        // Larger graph with deterministic weights
        let mut g = CompleteGraph::new(10);
        for i in 0..10 {
            for j in 0..10 {
                if i != j {
                    let weight = ((i * 10 + j) % 100 + 1) as u32;
                    g[(i, j)] = weight;
                }
            }
        }
        assert_eq!(mst_prim(&g).0, mst_prim_cost_only(&g));
    }

    #[test]
    fn test_mst_prim_with_fixed_subtour_empty() {
        let g: CompleteGraph<u32> = CompleteGraph::new(0);
        let (cost, mst) = mst_prim_with_fixed_subtour(&g, &[]);
        assert_eq!(cost, 0);
        assert!(mst.is_empty());
    }

    #[test]
    fn test_mst_prim_with_fixed_subtour_single_node() {
        let g: CompleteGraph<u32> = CompleteGraph::new(1);
        let (cost, mst) = mst_prim_with_fixed_subtour(&g, &[0]);
        assert_eq!(cost, 0);
        assert!(mst.is_empty());
    }

    #[test]
    fn test_mst_prim_with_fixed_subtour_full_subtour() {
        // All nodes in the subtour: MST should be empty and cost 0
        let mut g = CompleteGraph::new(3);
        g[(0, 1)] = 2u32;
        g[(1, 0)] = 2u32;
        g[(0, 2)] = 1u32;
        g[(2, 0)] = 1u32;
        g[(1, 2)] = 3u32;
        g[(2, 1)] = 3u32;
        let (cost, mst) = mst_prim_with_fixed_subtour(&g, &[0, 1, 2]);
        assert_eq!(cost, 0);
        assert!(mst.is_empty());
    }

    #[test]
    fn test_mst_prim_with_fixed_subtour_partial() {
        // Subtour is [0], MST should be the same as normal Prim
        let mut g = CompleteGraph::new(3);
        g[(0, 1)] = 2u32;
        g[(1, 0)] = 2u32;
        g[(0, 2)] = 1u32;
        g[(2, 0)] = 1u32;
        g[(1, 2)] = 3u32;
        g[(2, 1)] = 3u32;
        let (cost1, mst1) = mst_prim(&g);
        let (cost2, mst2) = mst_prim_with_fixed_subtour(&g, &[0]);
        assert_eq!(cost1, cost2);
        assert_eq!(mst1, mst2);
    }

    #[test]
    fn test_mst_prim_with_fixed_subtour_nontrivial() {
        // Subtour is [0, 1], MST should only connect 2 to the subtour
        let mut g = CompleteGraph::new(3);
        g[(0, 1)] = 2u32;
        g[(1, 0)] = 2u32;
        g[(0, 2)] = 1u32;
        g[(2, 0)] = 1u32;
        g[(1, 2)] = 3u32;
        g[(2, 1)] = 3u32;
        let (cost, mst) = mst_prim_with_fixed_subtour(&g, &[0, 1]);
        assert_eq!(cost, 1); // Only edge 0-2 (or 2-0) should be picked
        assert_eq!(mst.len(), 1);
        let edge = &mst[0];
        assert!((edge.source == 0 && edge.target == 2) || (edge.source == 2 && edge.target == 0));
        assert_eq!(edge.data, 1);
    }

    #[test]
    fn test_mst_prim_with_fixed_subtour_larger() {
        // Subtour is [0, 1], MST should connect 2 and 3 optimally
        let mut g = CompleteGraph::new(4);
        g[(0, 1)] = 1u32;
        g[(1, 0)] = 1u32;
        g[(0, 2)] = 2u32;
        g[(2, 0)] = 2u32;
        g[(0, 3)] = 3u32;
        g[(3, 0)] = 3u32;
        g[(1, 2)] = 4u32;
        g[(2, 1)] = 4u32;
        g[(1, 3)] = 5u32;
        g[(3, 1)] = 5u32;
        g[(2, 3)] = 6u32;
        g[(3, 2)] = 6u32;
        let (cost, mst) = mst_prim_with_fixed_subtour(&g, &[0, 1]);
        // MST should connect 2 to 0 (cost 2), 3 to 0 (cost 3)
        assert_eq!(cost, 5);
        assert_eq!(mst.len(), 2);
        let edge_set = mst
            .iter()
            .map(|e| (e.source.min(e.target), e.source.max(e.target), e.data))
            .collect::<std::collections::HashSet<_>>();
        let expected = [(0, 2, 2), (0, 3, 3)];
        for &(a, b, w) in &expected {
            assert!(edge_set.contains(&(a, b, w)));
        }
    }
}
