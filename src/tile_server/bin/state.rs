//! What the handlers are answered from, and the work that is done once per
//! level rather than once per tile.
//!
//! A cell has a convex hull, an alpha shape and a place in a tree, and none of
//! the three is cheap: a hull costs a walk of every node of the cell, and the
//! finest level of a continent holds half a million cells. None of it is worth
//! paying for per tile, so a level works all of it out the first time it is
//! asked for and holds on to it.

use rustc_hash::FxHashMap;
use std::sync::{Arc, Mutex};

use log::info;
use rayon::prelude::*;
use toolbox_rs::{
    alpha_shape::alpha_shape,
    bounding_box::BoundingBox,
    convex_hull::monotone_chain,
    customization::{CellDistances, Customization, Level},
    geometry::{FPCoordinate, Point2D},
    level_directory::{CellId, LevelDirectory},
    metric::{Metric, Scaled},
    nearest::{Indexed, NearestIndex},
    static_graph::StaticGraph,
};

use crate::tile_index::TileData;

/// How many metres a degree of latitude is worth, near enough for putting the
/// points of one cell on a plane.
pub(crate) const METRES_PER_DEGREE: f64 = 111_320.0;

/// The convex hull of a cell together with the corners of the box it covers.
pub type Hull = (Vec<FPCoordinate>, [FPCoordinate; 2]);

/// A cell of a level as the tree holds it: which cell it is and the box it
/// covers. The cells of one level are of one size, but the levels are not, and
/// a cell of the coarsest level covers a country. A grid of buckets holds that
/// badly, as such a cell lands in every bucket there is; a tree holds boxes of
/// any size and is why this is a tree.
#[derive(Clone, Copy, Debug)]
pub struct CellBox {
    pub cell: CellId,
    pub bbox: BoundingBox,
}

impl Indexed for CellBox {
    const BYTES: usize = 4 + 8 + 8;
    const TAG: u32 = 3;

    fn write_to(&self, into: &mut [u8]) {
        let (min, max) = self.bbox.corners();
        into[0..4].copy_from_slice(&self.cell.to_le_bytes());
        into[4..8].copy_from_slice(&min.lat.to_le_bytes());
        into[8..12].copy_from_slice(&min.lon.to_le_bytes());
        into[12..16].copy_from_slice(&max.lat.to_le_bytes());
        into[16..20].copy_from_slice(&max.lon.to_le_bytes());
    }

    fn read_from(from: &[u8]) -> Self {
        let word = |held: &[u8]| i32::from_le_bytes(held.try_into().expect("four bytes"));
        Self {
            cell: u32::from_le_bytes(from[0..4].try_into().expect("four bytes")),
            bbox: BoundingBox::between(
                FPCoordinate::new(word(&from[4..8]), word(&from[8..12])),
                FPCoordinate::new(word(&from[12..16]), word(&from[16..20])),
            ),
        }
    }

    fn bbox(&self) -> BoundingBox {
        self.bbox
    }

    fn center(&self) -> FPCoordinate {
        self.bbox.center()
    }

    /// The nearest point of the box, measured the way the walk measures the
    /// boxes above it, so that a cell is never handed out before a nearer one.
    fn nearest_to(&self, at: &FPCoordinate, by: &Scaled) -> (f64, FPCoordinate) {
        let near = self.bbox.nearest_point(at);
        (by.distance(&near, at), near)
    }
}

/// The alpha shape of a cell, as the rings of its boundary. A cell that falls
/// into pieces under the disc has one ring apiece.
pub type Shape = Vec<Vec<FPCoordinate>>;

/// Everything the handlers are answered from.
pub struct ServerState {
    pub tiles: TileData,
    pub coordinates: Vec<FPCoordinate>,
    /// the cells of the partition and what it costs to cross them
    pub customization: Customization,
    /// The alpha shape of each cell of a level, worked out the first time that
    /// level is asked for, as the rings of its boundary.
    pub shapes: Mutex<FxHashMap<usize, Arc<Vec<Shape>>>>,
    /// how large the disc that carves a shape out of a hull is, in metres
    pub alpha: f64,
    /// The cells of a level in a tree, so that a tile can ask which of them
    /// reach it rather than trying every one. A level of the finest cut holds
    /// half a million cells, and every tile was walking all of them.
    pub cell_trees: Mutex<FxHashMap<usize, Option<Arc<NearestIndex<CellBox>>>>>,
    /// The convex hull of each cell of a level, worked out the first time that
    /// level is asked for. A hull costs a walk of the nodes of the cell, which
    /// is not something to pay per tile.
    pub hulls: Mutex<FxHashMap<usize, Arc<Vec<Hull>>>>,
    /// the deepest level the partition carries, i.e. the level of its leaves
    pub max_level: u32,
}

impl ServerState {
    pub fn new(
        graph: StaticGraph<u32>,
        coordinates: Vec<FPCoordinate>,
        directory: LevelDirectory,
        alpha: f64,
    ) -> Self {
        // the finest level is the one that separates the most, so an arc that
        // stays inside a cell there stays inside one everywhere
        let finest = (0..directory.number_of_nodes())
            .map(|node| directory.cell_of(node, 0))
            .collect::<Vec<_>>();
        let tiles = TileData::new(&graph, &coordinates, &finest);
        let max_level = directory.levels() as u32 - 1;
        let watched = coordinates.clone();

        Self {
            tiles,
            coordinates,
            customization: Customization::new(graph, directory).watched_by(move |report| {
                // the box the cell covers, in the order a bbox is usually
                // written in, so that it can be pasted into a map
                let mut west = f64::MAX;
                let mut south = f64::MAX;
                let mut east = f64::MIN;
                let mut north = f64::MIN;
                for &node in report.nodes {
                    let (lon, lat) = watched[node].to_lon_lat_pair();
                    west = west.min(lon);
                    east = east.max(lon);
                    south = south.min(lat);
                    north = north.max(lat);
                }
                info!(
                    "customized cell {} on level {} in {:.1?}: {} nodes, {} of them on the border, searched over {}, bbox {west:.6},{south:.6},{east:.6},{north:.6}",
                    report.cell,
                    report.level,
                    report.elapsed,
                    report.nodes.len(),
                    report.border_nodes,
                    report.searched
                );
                info!(
                    "customization so far: {} cells in {:.1?}",
                    report.customized_cells, report.total
                );
            }),
            hulls: Mutex::new(FxHashMap::default()),
            shapes: Mutex::new(FxHashMap::default()),
            cell_trees: Mutex::new(FxHashMap::default()),
            alpha,
            max_level,
        }
    }

    /// the graph the partition was built over
    pub fn graph(&self) -> &StaticGraph<u32> {
        self.customization.graph()
    }

    /// which cell each node sits in on each level
    pub fn directory(&self) -> &LevelDirectory {
        self.customization.directory()
    }

    /// The cells of a level, worked out on the first request for it and kept.
    pub fn level(&self, level: usize) -> Arc<Level> {
        self.customization.level(level)
    }

    /// The alpha shape of every cell of a level, as the rings of its boundary,
    /// worked out on the first request for the level and kept.
    ///
    /// The points of a cell are put on a plane of metres before the disc is
    /// rolled around them, as a degree of longitude is shorter than one of
    /// latitude by the cosine of where you are and a shape worked out in raw
    /// degrees comes out stretched. A cell too large to triangulate keeps its
    /// convex hull, so the layer draws something for every cell either way.
    pub fn shapes(&self, level: usize) -> Arc<Vec<Shape>> {
        if let Some(shapes) = self
            .shapes
            .lock()
            .expect("the shape cache is poisoned")
            .get(&level)
        {
            return shapes.clone();
        }

        let cells = self.level(level);
        let hulls = self.hulls(level);
        let alpha = self.alpha;
        let shapes = (0..cells.cells())
            .into_par_iter()
            .map(|cell| {
                let nodes = cells.nodes_of(cell as CellId);
                if nodes.len() < 3 {
                    let hull = hulls[cell].0.clone();
                    return if hull.len() < 3 {
                        Vec::new()
                    } else {
                        vec![hull]
                    };
                }

                // a plane of metres about the middle of the cell
                let middle = nodes
                    .iter()
                    .map(|&node| self.coordinates[node].to_lon_lat_pair().1)
                    .sum::<f64>()
                    / nodes.len() as f64;
                let stretch = middle.to_radians().cos();
                let points = nodes
                    .iter()
                    .map(|&node| {
                        let (lon, lat) = self.coordinates[node].to_lon_lat_pair();
                        Point2D {
                            x: lon * stretch * METRES_PER_DEGREE,
                            y: lat * METRES_PER_DEGREE,
                        }
                    })
                    .collect::<Vec<_>>();

                alpha_shape(&points, alpha)
                    .into_iter()
                    .map(|ring| {
                        ring.into_iter()
                            .map(|at| self.coordinates[nodes[at]])
                            .collect()
                    })
                    .collect()
            })
            .collect::<Vec<_>>();

        let shapes = Arc::new(shapes);
        self.shapes
            .lock()
            .expect("the shape cache is poisoned")
            .entry(level)
            .or_insert(shapes)
            .clone()
    }

    /// The cells of a level in a tree, built from their hulls the first time
    /// the level is asked for and kept.
    ///
    /// Nothing for a level where no cell has a hull to speak of, as an index
    /// wants a box to be measured in and an empty one has none.
    pub fn cell_tree(&self, level: usize) -> Option<Arc<NearestIndex<CellBox>>> {
        if let Some(tree) = self
            .cell_trees
            .lock()
            .expect("the cell tree cache is poisoned")
            .get(&level)
        {
            return tree.clone();
        }

        let hulls = self.hulls(level);
        let boxes = hulls
            .iter()
            .enumerate()
            .filter_map(|(cell, (hull, corners))| {
                if hull.len() < 3 {
                    return None;
                }
                Some(CellBox {
                    cell: cell as CellId,
                    bbox: BoundingBox::from_coordinates(corners),
                })
            })
            .collect::<Vec<_>>();
        let tree = (!boxes.is_empty()).then(|| Arc::new(NearestIndex::over(boxes)));
        self.cell_trees
            .lock()
            .expect("the cell tree cache is poisoned")
            .entry(level)
            .or_insert(tree)
            .clone()
    }

    /// The convex hull of every cell of a level, with the box it covers, worked
    /// out on the first request for the level and kept. A hull is a property of
    /// a cell rather than of a tile, so it is worked out over the whole cell
    /// and cut down to whichever tile asks for it.
    pub fn hulls(&self, level: usize) -> Arc<Vec<Hull>> {
        if let Some(hulls) = self
            .hulls
            .lock()
            .expect("the hull cache is poisoned")
            .get(&level)
        {
            return hulls.clone();
        }

        let cells = self.level(level);
        let hulls = (0..cells.cells())
            .into_par_iter()
            .map(|cell| {
                let nodes = cells.nodes_of(cell as CellId);
                let coordinates = nodes
                    .iter()
                    .map(|&node| self.coordinates[node])
                    .collect::<Vec<_>>();
                let hull = monotone_chain(&coordinates);
                // the corners of the box the hull covers, kept so that a tile
                // can pass over a cell it does not reach without walking it
                let mut low = FPCoordinate::new(i32::MAX, i32::MAX);
                let mut high = FPCoordinate::new(i32::MIN, i32::MIN);
                for point in &hull {
                    low = FPCoordinate::new(low.lat.min(point.lat), low.lon.min(point.lon));
                    high = FPCoordinate::new(high.lat.max(point.lat), high.lon.max(point.lon));
                }
                (hull, [low, high])
            })
            .collect::<Vec<_>>();

        let hulls = Arc::new(hulls);
        self.hulls
            .lock()
            .expect("the hull cache is poisoned")
            .entry(level)
            .or_insert(hulls)
            .clone()
    }

    /// Hands out the distances of a cell, tabulating them on the first request.
    pub fn distances_of(&self, level: usize, cell: CellId) -> Option<&CellDistances> {
        self.customization.distances_of(level, cell)
    }

    /// The level a request is answered at: the one that was asked for, held
    /// within what the directory carries, and the finest when none was asked
    /// for. Level zero is the finest, and every level above it is coarser.
    pub fn level_or_finest(&self, level: Option<u32>) -> u32 {
        level.unwrap_or(0).min(self.max_level)
    }
}
