# Viewers

Pages for looking at what the crate produced, drawn in a browser because the
thing being looked at is a map.

## `search_space.html`

What a search over the cells looked at, with each level at its own height.

```
cargo run --release --example search_space -- \
    graph.toolbox levels.bin coordinates.toolbox <source> <target> \
    viewer/search_space.geojson

python3 -m http.server 8000
open http://localhost:8000/viewer/search_space.html
```

`?data=<path>` points it at another file.

Serve it rather than opening the file directly: reading the data is a fetch,
and a page opened off the filesystem is not allowed to make one.

The page needs the tab to be visible. A map draws on animation frames, and a
browser does not run those for a tab in the background, so a hidden tab sits
there with nothing on it. The page says so rather than leaving a black screen.

MapLibre is loaded from unpkg, so the page wants the network the first time.
There is no basemap: the tiles would say nothing this is about, and asking for
them means asking somebody else's server for permission to look at your own
data.
