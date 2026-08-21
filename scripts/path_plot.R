#!/usr/bin/env Rscript
#
# Draws unpacked paths over the network they run on.
#
#   Rscript scripts/path_plot.R paths.csv network.csv paths.png 12,16,19,21,23,24
#
# The inputs are what a run that unpacks paths writes out:
#
#   paths.csv    rank,step,lat,lon   every node of each way, in order
#   network.csv  rank,lat,lon        the nodes around that way
#
# The scatter is per path rather than one for the whole network. A way across a
# town and a way across a continent are four orders of magnitude apart, and a
# scatter thinned enough to draw the second leaves the first with nothing under
# it at all: the panel comes out empty and looks like a path drawn on air.
# Sampled to the window it is drawn in, every panel has a map under it.
#
# The last argument picks the ranks to draw, as the exponents; six of them fit
# the two by three the page is laid out in.
#
# Base graphics only, and no package beyond what R ships with, as with the
# plots beside it.

args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 3) {
  stop("usage: path_plot.R <paths.csv> <network.csv> [out.png] [ranks]")
}
paths <- read.csv(args[1])
network <- read.csv(args[2])
output <- if (length(args) >= 3) args[3] else "paths.png"
for (column in c("rank", "step", "lat", "lon")) {
  if (!column %in% names(paths)) {
    stop(sprintf("%s has no %s column", args[1], column))
  }
}

drawn <- if (length(args) >= 4) {
  as.integer(strsplit(args[4], ",")[[1]])
} else {
  # a spread over what there is, rather than the first six
  ranks <- sort(unique(paths$rank))
  ranks[unique(round(seq(1, length(ranks), length.out = 6)))]
}
drawn <- drawn[drawn %in% paths$rank]
if (length(drawn) == 0) {
  stop("none of the ranks asked for are in the file")
}

png(output, width = 1800, height = 1200, res = 130)
par(mfrow = c(2, ceiling(length(drawn) / 2)), mar = c(2, 2.5, 2.5, 1), las = 1)

for (rank in drawn) {
  way <- paths[paths$rank == rank, ]
  way <- way[order(way$step), ]
  # the same window the scatter was sampled to
  pad <- max(diff(range(way$lon)), diff(range(way$lat))) * 0.25 + 0.05
  xlim <- c(min(way$lon) - pad, max(way$lon) + pad)
  ylim <- c(min(way$lat) - pad, max(way$lat) + pad)

  # degrees of longitude are shorter than degrees of latitude away from the
  # equator, and a way drawn without saying so leans
  plot(NA,
    xlim = xlim, ylim = ylim, xlab = "", ylab = "",
    asp = 1 / cos(mean(ylim) * pi / 180),
    main = sprintf("rank 2^%d, %d nodes", rank, nrow(way))
  )
  near <- network[network$rank == rank, ]
  if (nrow(near) > 0) {
    # dense where the network is dense, which is what makes a town read as a
    # town; the alpha carries it rather than the size
    points(near$lon, near$lat, pch = 19, cex = 0.14, col = "#00000022")
  }
  # the straight line between the ends, to read the way against
  lines(c(way$lon[1], way$lon[nrow(way)]), c(way$lat[1], way$lat[nrow(way)]),
        col = "#9a9a9a", lty = 2)
  lines(way$lon, way$lat, col = "#c05030", lwd = 1.9)
  points(way$lon[c(1, nrow(way))], way$lat[c(1, nrow(way))],
         pch = 19, col = c("#3060c0", "#309050"), cex = 1.2)
}

invisible(dev.off())
cat(sprintf("wrote %s: %d paths, ranks 2^%s\n",
            output, length(drawn), paste(drawn, collapse = ", 2^")))
