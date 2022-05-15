![GitHub last commit](https://img.shields.io/github/last-commit/DennisOSRM/toolbox-rs.svg)
![Crates.io](https://img.shields.io/crates/v/toolbox-rs.svg)

# Toolbox-rs
A toolbox of basic data structures and algorithms. If you heard of OSRM, please draw your own conclusions. 😁

## Chipper
A tool to bisect graphs in the DIMACS format using an implementation of the Inertial Flow method. Example graphs can be downloaded on the website of the [9th DIMACS implemenation challenge](http://www.diag.uniroma1.it//challenge9/download.shtml). Chipper reproduces the runtime and quality numbers reported by [Schild and Sommer (2015)](http://sommer.jp/roadseparator.pdf). Currently, a balance factor of 0.25 is the default, and can be overidden via the command line.

Usage via cargo:

```
$ cargo r --release --bin chipper -- -g /path/to/USA-road-t.USA.gr -c /path/to/USA-road-d.USA.co -o /path/to/result.txt
```
