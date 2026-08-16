//! Where the arcs and the border nodes of a partition are, arranged so that a
//! tile can ask for the ones that reach it.
//!
//! A cut of the finest level of a continent holds a million and a half arcs.
//! Walking all of them for every tile is what this exists to stop: every arc
//! and every border node is filed under the tile of [`INDEX_ZOOM`] it falls
//! into, and a request then reads the handful of buckets its tile covers.
//!
//! An arc that runs clean over a bucket without ending in it is filed under
//! that bucket too, or a ferry across open water would be drawn by nobody.

use log::info;
use rustc_hash::FxHashMap;
use toolbox_rs::{
    geometry::FPCoordinate,
    graph::{Graph, NodeID},
    level_directory::CellId,
    static_graph::StaticGraph,
    vector_tile::{TILE_SIZE, coordinate_to_tile_number, degree_to_pixel_lat, degree_to_pixel_lon},
    wgs84::{FloatCoordinate, FloatLatitude, FloatLongitude},
};

/// The zoom level the arcs are bucketed by. A request at this level or above
/// falls into exactly one bucket, which is all that has to be looked at. The
/// client asks for nothing below it.
pub const INDEX_ZOOM: u32 = 12;

/// An arc of the graph whose endpoints lie in different cells, i.e. a piece of
/// the boundary between two cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundaryArc {
    pub source: FPCoordinate,
    pub target: FPCoordinate,
    /// the nodes it runs between, so that the cell it separates can be read off
    /// whichever level is being drawn
    pub from: NodeID,
    pub to: NodeID,
}

/// A node that an arc leaves its cell on. The distances between the border
/// nodes of a cell are what a cell is summarized by, so these are the nodes
/// worth asking about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BorderNode {
    pub coordinate: FPCoordinate,
    pub node: NodeID,
}

/// The part of the input the tile requests are answered from.
pub struct TileData {
    pub boundary: Vec<BoundaryArc>,
    pub border_nodes: Vec<BorderNode>,
    /// the nodes that fall into a tile of [`INDEX_ZOOM`], so that a request
    /// only has to look at the arcs that can reach it
    pub nodes_by_tile: FxHashMap<(u32, u32), Vec<NodeID>>,
    /// the bucket of each node, packed by [`pack_bucket`]
    pub bucket_of_node: Vec<u32>,
    /// the arcs of the cut that reach into a bucket, as offsets into
    /// `boundary`. An arc is listed under every bucket it passes through, so a
    /// request has to weed out the ones it meets twice.
    pub boundary_by_tile: FxHashMap<(u32, u32), Vec<u32>>,
    /// which border nodes fall into each bucket, as offsets into
    /// `border_nodes`. Walking all of them per tile is nearly three million
    /// steps to find the few hundred that are on it.
    pub border_by_tile: FxHashMap<(u32, u32), Vec<u32>>,
    /// The arcs that span more than one bucket, as the pair of nodes they run
    /// between. An arc that ends in a bucket is found through the nodes of that
    /// bucket; one that only passes through is not, and this is what finds it.
    pub crossing: Vec<(NodeID, NodeID)>,
    /// which of those arcs pass through each bucket, as offsets into `crossing`
    pub crossing_by_tile: FxHashMap<(u32, u32), Vec<u32>>,
}

/// The range of [`INDEX_ZOOM`] tiles that the given tile covers, as the corners
/// of a rectangle of buckets, both ends included.
///
/// A tile of a zoom level above the index falls into a single bucket. One below
/// it spans four buckets per level of the difference, and every one of them has
/// to be looked at or only a fraction of the arcs is drawn.
pub fn index_tiles_of(zoom: u32, x: u32, y: u32) -> (u32, u32, u32, u32) {
    if zoom >= INDEX_ZOOM {
        let down = zoom - INDEX_ZOOM;
        let (x, y) = (x >> down, y >> down);
        (x, y, x, y)
    } else {
        let up = INDEX_ZOOM - zoom;
        let span = (1 << up) - 1;
        (x << up, y << up, (x << up) + span, (y << up) + span)
    }
}

/// Where a coordinate falls in the grid of index buckets, as a fraction rather
/// than as the bucket it lands in, so that a segment can be walked across the
/// grid rather than only asked where it ends.
pub fn bucket_place_of(coordinate: FPCoordinate) -> (f64, f64) {
    let (lon, lat) = coordinate.to_lon_lat_pair();
    let side = TILE_SIZE as f64;
    let across = degree_to_pixel_lon(FloatLongitude(lon), INDEX_ZOOM) / side;
    let down = degree_to_pixel_lat(FloatLatitude(lat), INDEX_ZOOM) / side;
    (across, down)
}

/// Every bucket a segment passes through, not only the two it ends in.
///
/// An arc that crosses a bucket without ending in it belongs to that bucket as
/// much as one that starts there: a tile is drawn from the buckets it covers,
/// and an arc listed only under its ends is missing from every tile in between.
/// A ferry is the case that shows it, running an hour of open water across
/// buckets that hold nothing else.
///
/// This walks the grid the way a ray does, stepping to whichever side comes
/// next, so it visits each bucket the segment touches once.
pub fn buckets_crossed(from: FPCoordinate, to: FPCoordinate) -> Vec<(u32, u32)> {
    let (from_x, from_y) = bucket_place_of(from);
    let (to_x, to_y) = bucket_place_of(to);
    let mut at = (from_x.floor() as i64, from_y.floor() as i64);
    let last = (to_x.floor() as i64, to_y.floor() as i64);

    let mut crossed = vec![at];
    if at == last {
        return crossed
            .drain(..)
            .map(|(x, y)| (x as u32, y as u32))
            .collect();
    }

    let (dx, dy) = (to_x - from_x, to_y - from_y);
    let step = (dx.signum() as i64, dy.signum() as i64);
    // how far along the segment the next side of the bucket lies, and how far
    // apart two sides are, in the same units
    let next_x = if dx > 0.0 {
        (at.0 as f64 + 1.0 - from_x) / dx
    } else if dx < 0.0 {
        (at.0 as f64 - from_x) / dx
    } else {
        f64::INFINITY
    };
    let next_y = if dy > 0.0 {
        (at.1 as f64 + 1.0 - from_y) / dy
    } else if dy < 0.0 {
        (at.1 as f64 - from_y) / dy
    } else {
        f64::INFINITY
    };
    let (mut along_x, mut along_y) = (next_x, next_y);
    let (apart_x, apart_y) = (
        if dx == 0.0 {
            f64::INFINITY
        } else {
            1.0 / dx.abs()
        },
        if dy == 0.0 {
            f64::INFINITY
        } else {
            1.0 / dy.abs()
        },
    );

    // a segment cannot touch more buckets than the sides it crosses, and the
    // cap is what keeps a coordinate that came out wrong from spinning here
    for _ in 0..4096 {
        if along_x < along_y {
            at.0 += step.0;
            along_x += apart_x;
        } else {
            at.1 += step.1;
            along_y += apart_y;
        }
        crossed.push(at);
        if at == last {
            break;
        }
    }
    crossed
        .into_iter()
        .filter(|&(x, y)| x >= 0 && y >= 0)
        .map(|(x, y)| (x as u32, y as u32))
        .collect()
}

/// The bucket of a node, packed into one number. A tile number of
/// [`INDEX_ZOOM`] needs twelve bits, so both of them fit next to each other and
/// the bucket of every node costs four bytes rather than eight.
pub const fn pack_bucket(x: u32, y: u32) -> u32 {
    (x << INDEX_ZOOM) | y
}

impl TileData {
    /// Collects the arcs that leave their cell together with the nodes they
    /// leave on. Those arcs are what separates one cell from the next, so
    /// drawing them draws the partition. Each pair of nodes is taken once, as
    /// the graph holds both directions of an arc.
    pub fn new(graph: &StaticGraph<usize>, coordinates: &[FPCoordinate], cells: &[CellId]) -> Self {
        let mut boundary = Vec::new();
        let mut border_nodes = Vec::new();
        let mut nodes_by_tile: FxHashMap<(u32, u32), Vec<NodeID>> = FxHashMap::default();
        let mut bucket_of_node = vec![0; graph.number_of_nodes()];
        for source in graph.node_range() {
            let (lon, lat) = coordinates[source].to_lon_lat_pair();
            let tile = coordinate_to_tile_number(
                FloatCoordinate {
                    lat: FloatLatitude(lat),
                    lon: FloatLongitude(lon),
                },
                INDEX_ZOOM,
            );
            nodes_by_tile.entry(tile).or_default().push(source);
            bucket_of_node[source] = pack_bucket(tile.0, tile.1);

            let mut leaves_cell = false;
            for edge in graph.edge_range(source) {
                let target = graph.target(edge);
                if cells[source] == cells[target] {
                    continue;
                }
                leaves_cell = true;
                // the reverse of this arc carries the same segment
                if source < target {
                    boundary.push(BoundaryArc {
                        source: coordinates[source],
                        target: coordinates[target],
                        from: source,
                        to: target,
                    });
                }
            }
            if leaves_cell {
                border_nodes.push(BorderNode {
                    coordinate: coordinates[source],
                    node: source,
                });
            }
        }
        // The cut is indexed by every bucket its arcs pass through rather than
        // by the two they end in. An arc listed only under its ends is missing
        // from every tile in between, which is what a ferry crossing open water
        // is: an hour of it over buckets that hold nothing else.
        let mut boundary_by_tile: FxHashMap<(u32, u32), Vec<u32>> = FxHashMap::default();
        for (offset, arc) in boundary.iter().enumerate() {
            let offset = u32::try_from(offset).expect("more arcs on the cut than fit into u32");
            for tile in buckets_crossed(arc.source, arc.target) {
                boundary_by_tile.entry(tile).or_default().push(offset);
            }
        }
        for arcs in boundary_by_tile.values_mut() {
            arcs.sort_unstable();
            arcs.dedup();
            arcs.shrink_to_fit();
        }

        let mut border_by_tile: FxHashMap<(u32, u32), Vec<u32>> = FxHashMap::default();
        for (offset, border) in border_nodes.iter().enumerate() {
            let offset = u32::try_from(offset).expect("more border nodes than fit into u32");
            let bucket = bucket_of_node[border.node];
            let tile = (bucket >> INDEX_ZOOM, bucket & ((1 << INDEX_ZOOM) - 1));
            border_by_tile.entry(tile).or_default().push(offset);
        }
        for nodes in border_by_tile.values_mut() {
            nodes.shrink_to_fit();
        }

        // The arcs that span more than one bucket, listed under the buckets
        // they only pass through. A tile is drawn from the nodes of the buckets
        // it covers, so an arc with neither end in any of them is drawn by
        // nobody, however far across the tile it runs.
        let mut crossing = Vec::new();
        let mut crossing_by_tile: FxHashMap<(u32, u32), Vec<u32>> = FxHashMap::default();
        for source in graph.node_range() {
            for edge in graph.edge_range(source) {
                let target = graph.target(edge);
                // the graph holds both directions and the segment is the same
                if target <= source {
                    continue;
                }
                let through = buckets_crossed(coordinates[source], coordinates[target]);
                if through.len() < 3 {
                    // it ends in both of the buckets it touches, so the nodes
                    // of those buckets already find it
                    continue;
                }
                let offset =
                    u32::try_from(crossing.len()).expect("more long arcs than fit into u32");
                let (from, to) = (
                    (
                        bucket_of_node[source] >> INDEX_ZOOM,
                        bucket_of_node[source] & ((1 << INDEX_ZOOM) - 1),
                    ),
                    (
                        bucket_of_node[target] >> INDEX_ZOOM,
                        bucket_of_node[target] & ((1 << INDEX_ZOOM) - 1),
                    ),
                );
                let mut listed = false;
                for tile in through {
                    if tile == from || tile == to {
                        continue;
                    }
                    crossing_by_tile.entry(tile).or_default().push(offset);
                    listed = true;
                }
                if listed {
                    crossing.push((source, target));
                }
            }
        }
        for arcs in crossing_by_tile.values_mut() {
            arcs.sort_unstable();
            arcs.dedup();
            arcs.shrink_to_fit();
        }
        info!(
            "{} arcs run through a bucket without ending in it",
            crossing.len()
        );
        crossing.shrink_to_fit();

        boundary.shrink_to_fit();
        border_nodes.shrink_to_fit();
        for nodes in nodes_by_tile.values_mut() {
            // the nodes were collected in order, which is what the membership
            // test of the interior arcs relies on
            debug_assert!(nodes.windows(2).all(|pair| pair[0] < pair[1]));
            nodes.shrink_to_fit();
        }
        Self {
            boundary,
            border_nodes,
            nodes_by_tile,
            bucket_of_node,
            border_by_tile,
            crossing,
            crossing_by_tile,
            boundary_by_tile,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolbox_rs::edge::InputEdge;

    #[test]
    fn boundary_holds_the_arcs_that_leave_their_cell() {
        // 0 - 1 - 2, with the cut between node 1 and node 2
        let edges = vec![
            InputEdge::new(0, 1, 1_usize),
            InputEdge::new(1, 0, 1_usize),
            InputEdge::new(1, 2, 1_usize),
            InputEdge::new(2, 1, 1_usize),
        ];
        let graph = StaticGraph::new(edges);
        let coordinates = vec![
            FPCoordinate::new_from_lat_lon(50.0, 8.0),
            FPCoordinate::new_from_lat_lon(50.1, 8.1),
            FPCoordinate::new_from_lat_lon(50.2, 8.2),
        ];
        let data = TileData::new(&graph, &coordinates, &[0, 0, 1]);

        // only the arc between the cells is on the boundary, and it is taken
        // once although the graph holds both of its directions
        assert_eq!(data.boundary.len(), 1);
        assert_eq!(data.boundary[0].source, coordinates[1]);
        assert_eq!(data.boundary[0].target, coordinates[2]);
        assert_eq!(data.boundary[0].from, 1);
        assert_eq!(data.boundary[0].to, 2);
    }

    #[test]
    fn a_partition_of_one_cell_has_no_boundary() {
        let edges = vec![InputEdge::new(0, 1, 1_usize), InputEdge::new(1, 0, 1_usize)];
        let graph = StaticGraph::new(edges);
        let coordinates = vec![
            FPCoordinate::new_from_lat_lon(50.0, 8.0),
            FPCoordinate::new_from_lat_lon(50.1, 8.1),
        ];
        assert!(
            TileData::new(&graph, &coordinates, &[0, 0])
                .boundary
                .is_empty()
        );
    }
}
