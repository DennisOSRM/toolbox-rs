#!/usr/bin/env Rscript
#
# Draws what the searches cost against how far apart the ends of a query are.
#
#   Rscript scripts/rank_plot.R timings.csv ranks.png
#
# The input is what `ranks time` writes, with the runs of both engines laid end
# to end:
#
#   ranks time -g graph -i pairs.csv -e dijkstra -o d.csv
#   ranks time -g graph -d levels -i pairs.csv -e mld -o m.csv
#   cat d.csv <(tail -n +2 m.csv) > timings.csv
#
# Base graphics only, and no package beyond what R ships with. A plot that
# wants an install first is a plot that does not get looked at, and this one is
# meant to run wherever the numbers were made.

args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) {
  stop("usage: rank_plot.R <timings.csv> [out.png]")
}
input <- args[1]
output <- if (length(args) >= 2) args[2] else "ranks.png"

timings <- read.csv(input, stringsAsFactors = FALSE)
for (column in c("engine", "rank", "nanos")) {
  if (!column %in% names(timings)) {
    stop(sprintf("%s has no %s column", input, column))
  }
}

timings <- timings[timings$rank > 0 & timings$nanos > 0, ]
if (nrow(timings) == 0) {
  stop("there is nothing to plot")
}

# microseconds read better than nanoseconds, and the rank axis is drawn at the
# exponent so the ticks say 2^10 rather than 1024
timings$micros <- timings$nanos / 1000
timings$exponent <- round(log2(timings$rank))
engines <- sort(unique(timings$engine))
exponents <- sort(unique(timings$exponent))

# a bucket holding a handful of samples is drawn like the rest and marked, so
# that a thin tail reads as a thin tail rather than as a result
THIN <- 5
counts <- table(timings$exponent)
thin <- as.integer(names(counts)[counts < THIN * length(engines)])

palette <- c("#3060c0", "#c05030", "#309050", "#9050b0")
colour_of <- setNames(palette[seq_along(engines)], engines)

png(output, width = 1400, height = 1000, res = 130)
layout(matrix(c(1, 2), nrow = 2), heights = c(2, 1))
par(mar = c(1.5, 4.5, 2.5, 1), las = 1)

# what each engine costs, as a box per bucket
groups <- list()
at <- c()
fill <- c()
width <- 0.8 / length(engines)
for (index in seq_along(exponents)) {
  for (which in seq_along(engines)) {
    rows <- timings$exponent == exponents[index] & timings$engine == engines[which]
    groups[[length(groups) + 1]] <- timings$micros[rows]
    offset <- (which - (length(engines) + 1) / 2) * width
    at <- c(at, exponents[index] + offset)
    fill <- c(fill, colour_of[engines[which]])
  }
}

boxplot(groups,
  at = at, boxwex = width * 0.9, col = fill, log = "y",
  xaxt = "n", xlab = "", ylab = "microseconds",
  main = sprintf("query time by Dijkstra rank (%s)", basename(input)),
  outcex = 0.3, whisklty = 1, staplewex = 0.5
)
axis(1, at = exponents, labels = parse(text = sprintf("2^%d", exponents)))
abline(v = exponents[-1] - 0.5, col = "#00000012")
legend("topleft", legend = engines, fill = colour_of[engines], bty = "n")
if (length(thin) > 0) {
  mtext(sprintf("thin buckets, under %d samples: 2^%s", THIN,
                paste(thin, collapse = ", 2^")),
        side = 3, line = 0, cex = 0.7, col = "#a04000")
}

# and what one is worth against another, which is the curve to read
par(mar = c(4.5, 4.5, 1, 1))
medians <- sapply(engines, function(engine) {
  sapply(exponents, function(exponent) {
    rows <- timings$exponent == exponent & timings$engine == engine
    if (any(rows)) median(timings$micros[rows]) else NA
  })
})
medians <- matrix(medians, nrow = length(exponents), dimnames = list(NULL, engines))

if (all(c("dijkstra", "mld") %in% engines)) {
  speedup <- medians[, "dijkstra"] / medians[, "mld"]
  ylim <- range(c(speedup, 1), na.rm = TRUE)
  plot(exponents, speedup,
    type = "b", pch = 19, log = "y", ylim = ylim, xaxt = "n",
    xlab = "Dijkstra rank", ylab = "dijkstra / mld", col = "#204080"
  )
  axis(1, at = exponents, labels = parse(text = sprintf("2^%d", exponents)))
  # at one the two cost the same; below it the cells are not paying for
  # themselves, and it should climb as more of them can be stepped over. A
  # curve that does not climb is the thing to look for: every level is sound to
  # step over, so a query that picks a low one gives the same answers and no
  # test of those answers would say a word.
  abline(h = 1, lty = 2, col = "#808080")
  best <- which.max(speedup)
  if (length(best) == 1 && is.finite(speedup[best])) {
    mtext(sprintf("best %.1fx at 2^%d", speedup[best], exponents[best]),
          side = 3, line = -1.2, adj = 0.98, cex = 0.75, col = "#204080")
  }
} else {
  plot.new()
  mtext("a speedup wants both engines in the file", side = 3, line = -3,
        cex = 0.8, col = "#808080")
}

invisible(dev.off())
cat(sprintf("wrote %s: %d timings, %d engines, ranks 2^%d to 2^%d\n",
            output, nrow(timings), length(engines),
            min(exponents), max(exponents)))
