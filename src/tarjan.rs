// use std::cmp::min;

// struct Node {
//   index: usize,
//   low_link: usize,
// }

// impl Node {
//     fn new(index: usize, low_link: usize) -> Self { Self { index, low_link } }

//     fn update_low_link(&mut self, update: usize) {
//       self.low_link = min(self.low_link, update);
//     }
// }

// pub fn tarjan<G>(graph: &G) {

//   #[derive(Clone)]
//   enum ExplorationState {
//     Unseen,
//     Unexplored,
//     Completed
//   }

//   let mut state = vec![ExplorationState::Unseen; graph.number_of_nodes()];
//   let mut stack = Vec::new();

//   let mut index = 0;
//   for root in graph.node_range() {
//     if

//     stack.push(Node::new(0, 0));

//     while !stack.is_empty() {

//     }
//   }

// }

// // # Tarjan's algorithm.
// // def sconnect(v):
// // global next, nextgroup
// // work = [(v, 0)] # NEW: Recursion stack.
// // while work:
// //     v, i = work[-1] # i is next successor to process.
// //     del work[-1]
// //     if i == 0: # When first visiting a vertex:
// //         index[v] = next
// //         lowlink[v] = next
// //         next += 1
// //         stack.append(v)
// //         onstack[v] = True
// //     recurse = False
// //     for j in range(i, len(adj[v])):
// //         w = adj[v][j]
// //         if index[w] == None:
// //             # CHANGED: Add w to recursion stack.
// //             work.append((v, j+1))
// //             work.append((w, 0))
// //             recurse = True
// //             break
// //         elif onstack[w]:
// //             lowlink[v] = min(lowlink[v], index[w])
// //     if recurse: continue # NEW
// //     if index[v] == lowlink[v]:
// //         com = []
// //         while True:
// //             w = stack[-1]
// //             del stack[-1]
// //             onstack[w] = False
// //             com.append(w)
// //             groupid[w] = nextgroup
// //             if w == v: break
// //         groups.append(com)
// //         nextgroup += 1
// //     if work: # NEW: v was recursively visited.
// //         w = v
// //         v, _ = work[-1]
// //         lowlink[v] = min(lowlink[v], lowlink[w])
