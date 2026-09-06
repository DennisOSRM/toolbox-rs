//! Whether a table read off a file is the table that was written.
//!
//!     edge_based_store_check <graph> <coordinates> <level directory> <tables>
//!
//! The cells are tabulated in memory and read back out of the store, and the
//! two are held against each other cell by cell: the arcs each names on its
//! border, and then what it says about crossing between them.
use std::env::args;

use toolbox_rs::{
    block_map::BlockMap,
    block_store::BlockStore,
    border_levels::BorderLevels,
    cell_tree::CellTree,
    edge_based::AnglePenalty,
    edge_based_overlay::{EdgeBasedOverlay, PagedEdgeBasedOverlay},
    geometry::FPCoordinate,
    io,
    level_directory::{CellId, LevelDirectory},
    overlay::{CellTable, Overlay},
    packed_partition::PackedPartition,
    paged_overlay::PagedOverlay,
    pool::Pool,
    static_graph::StaticGraph,
};

fn main() {
    let graph_at = args().nth(1).expect("a graph");
    let coordinates_at = args().nth(2).expect("coordinates");
    let directory_at = args().nth(3).expect("a level directory");
    let tables_at = args().nth(4).expect("the packed tables");

    let graph = StaticGraph::new(io::read_edges_from_file(&graph_at));
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&coordinates_at);
    let directory: LevelDirectory = io::read_from_file(&directory_at);

    let held = EdgeBasedOverlay::new(
        StaticGraph::new(io::read_edges_from_file(&graph_at)),
        coordinates.clone(),
        AnglePenalty::new(30., 100),
        &directory,
    );

    let map: BlockMap = io::read_from_file(&format!("{tables_at}.map"));
    let tree: CellTree = io::read_from_file(&format!("{tables_at}.tree"));
    let begins: Vec<Vec<u32>> = io::read_vec_from_file(&format!("{tables_at}.begins"));
    let widths: Vec<Vec<u32>> = io::read_vec_from_file(&format!("{tables_at}.widths"));
    let partition = PackedPartition::of(&directory);
    let borders = BorderLevels::of(&graph, &partition);
    let store = BlockStore::open_counting_from(
        std::path::Path::new(&format!("{tables_at}.blocks")),
        map,
        tree,
        begins.clone(),
        widths,
    )
    .expect("a store to open");
    let paged = PagedEdgeBasedOverlay::new(
        PagedOverlay::new(
            store,
            StaticGraph::new(io::read_edges_from_file(&graph_at)),
            partition,
            borders,
            Pool::of(512 << 20),
        ),
        graph,
        coordinates,
        AnglePenalty::new(30., 100),
    );

    println!(
        "{:>6} {:>10} {:>10} {:>10} {:>12} {:>12}",
        "level", "checked", "no table", "wrong ids", "wrong rows", "arcs begin"
    );
    for level in 0..directory.levels() {
        let cells = held.cells_on_level(level);
        let (mut checked, mut missing, mut wrong_ids, mut wrong_rows) = (0, 0, 0, 0);
        // a sample, since a continent has half a million cells on its finest
        for cell in (0..cells).step_by(1 + cells / 200) {
            let cell = cell as CellId;
            let (Some(mine), Some(theirs)) = (
                held.distances_of(level, cell),
                paged.distances_of(level, cell),
            ) else {
                missing += 1;
                continue;
            };
            checked += 1;
            if mine.border_nodes() != theirs.border_nodes() {
                wrong_ids += 1;
                if wrong_ids == 1 {
                    let (a, b) = (mine.border_nodes(), theirs.border_nodes());
                    let at = a.iter().zip(b).position(|(x, y)| x != y);
                    println!(
                        "  level {level} cell {cell}: held {} ids, read {} ids, first differing at {at:?}",
                        a.len(),
                        b.len()
                    );
                    if let Some(at) = at {
                        let upto = (at + 4).min(a.len()).min(b.len());
                        println!(
                            "    held {:?} against read {:?}",
                            &a[at..upto],
                            &b[at..upto]
                        );
                    }
                }
                continue;
            }
            for source in 0..mine.border_nodes().len() {
                if mine.row(source) != theirs.row(source) {
                    wrong_rows += 1;
                    break;
                }
            }
        }
        println!(
            "{level:>6} {checked:>10} {missing:>10} {wrong_ids:>10} {wrong_rows:>12} {:>12}",
            begins
                .get(level)
                .and_then(|at| at.first())
                .copied()
                .unwrap_or(0)
        );
    }
}
