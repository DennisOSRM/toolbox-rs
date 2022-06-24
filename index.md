## The Toolbox 🧰 🦀

### Jun, 24th 2022: Fixing a scalability issue

The recursive bi-partitioning exhibited a flaw in the amount of memory it allocated. The following graph shows how the performance regressed for levels greater or equal to 8. The graph shoots up exponentially. The issue was fixed in [#89](https://github.com/DennisOSRM/toolbox-rs/pull/89) by making sure that the per sub-graph memory allocation is independent of the overall graph size but only depends the size of the subgraph. Note that the speedup of the fixed version is super-liner.

![D4D05194-BECC-4F38-9FE5-4D07C396A7DD](https://user-images.githubusercontent.com/1067895/173334384-126b2c98-f318-4892-9b95-57f125dc9313.jpeg)
