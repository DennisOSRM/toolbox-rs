![GitHub last commit](https://img.shields.io/github/last-commit/DennisOSRM/toolbox-rs.svg)
![Crates.io](https://img.shields.io/crates/v/toolbox-rs.svg)

![Cells](https://user-images.githubusercontent.com/1067895/169662031-a2a516df-296e-42de-8095-d2a5ff5da3c6.JPG)

# Toolbox-rs
A toolbox of basic data structures and algorithms. If you heard of OSRM, please draw your own conclusions. 😁

## Chipper
A tool to bisect graphs in the DIMACS format using an implementation of the Inertial Flow method. Example graphs can be downloaded on the website of the [9th DIMACS implemenation challenge](http://www.diag.uniroma1.it//challenge9/download.shtml). Chipper reproduces the runtime and quality numbers reported by [Schild and Sommer (2015)](http://sommer.jp/roadseparator.pdf). Currently, a balance factor of 0.25 is the default, and can be overidden via the command line.

Usage via cargo:

```
$ cargo r --release --bin chipper -- -g /path/to/USA-road-t.USA.gr -c /path/to/USA-road-d.USA.co -o /path/to/result.txt -r30 -m100 -p /path/to/USA-r30-m100.assignment.bin
```

## Scaffold
A tool to generate run-time data structures from preprocess graph. At this point it supports visualizing cells by their convex hulls. The result of this is stored in GeoJSON format which can be easily visualized, e.g. on [Kepler.gl](https://kepler.gl/demo).

```
$ cargo r --release --bin scaffold -- -p /path/to/USA-r20-m100.assignment.bin -c /path/to/USA-road-d.USA.co  --convex-cells-geojson /path/to/bbox.geojson
```

## Visualizing Convex Hulls
![Convex Hulls](https://user-images.githubusercontent.com/1067895/175577261-55e38f44-07ae-4ab2-b344-23d15f5d5c89.png)
