#!/usr/bin/env Rscript
#
# Draws what each search is worth against a plain one, rank by rank.
#
#   Rscript scripts/speedup_plot.R timings.csv speedup.png
#
# The input is what `ranks time` writes, with the runs laid end to end and one
# of them a plain unidirectional search:
#
#   ranks time -g graph -i pairs.csv -e dijkstra -o d.csv
#   ranks time -g graph -d levels -i pairs.csv -e mld --warmup 4800 -o m.csv
#   ranks time -g graph -d levels -i pairs.csv -e bidirectional-mld --warmup 4800 -o b.csv
#   cat d.csv <(tail -n +2 m.csv) <(tail -n +2 b.csv) > timings.csv
#
# `rank_plot.R` draws the same numbers as boxes, and divides everything in the
# file by the engine named `mld`. That answers what one engine is worth against
# the others; this answers what each of them is worth against the plain search
# the rank axis is defined by, which is the curve a paper reports. Two engines
# over the cells therefore get a curve apiece rather than one being made the
# yardstick for the other.
#
# Base graphics only, and no package beyond what R ships with.

args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) {
  stop("usage: speedup_plot.R <timings.csv> [out.png] [baseline-engine]")
}
input <- args[1]
output <- if (length(args) >= 2) args[2] else "speedup.png"
baseline <- if (length(args) >= 3) args[3] else "dijkstra"

timings <- read.csv(input, stringsAsFactors = FALSE)
for (column in c("engine", "rank", "nanos")) {
  if (!column %in% names(timings)) {
    stop(sprintf("%s has no %s column", input, column))
  }
}
timings <- timings[timings$rank > 0 & timings$nanos > 0, ]
if (!baseline %in% timings$engine) {
  stop(sprintf("%s holds no %s run to measure against", input, baseline))
}

timings$exponent <- round(log2(timings$rank))
exponents <- sort(unique(timings$exponent))
against <- setdiff(sort(unique(timings$engine)), baseline)
if (length(against) == 0) {
  stop("there is nothing in the file but the baseline")
}

display <- c(dijkstra = "unidirectional", bidirectional = "bidirectional",
             mld = "mld", `bidirectional-mld` = "mld from both ends")
name_of <- function(engine) ifelse(engine %in% names(display), display[engine], engine)

# the median of a bucket rather than the mean: a query that happened to land
# while the machine was busy elsewhere should not move the curve
median_of <- function(engine) {
  sapply(exponents, function(exponent) {
    rows <- timings$exponent == exponent & timings$engine == engine
    if (any(rows)) median(timings$nanos[rows]) else NA
  })
}
plain <- median_of(baseline)
speedups <- sapply(against, function(engine) plain / median_of(engine))
speedups <- matrix(speedups, nrow = length(exponents), dimnames = list(NULL, against))

# a bucket holding a handful of samples is marked, so that a thin tail reads as
# a thin tail rather than as a result
THIN <- 5
counts <- table(timings$exponent)
thin <- as.integer(names(counts)[counts < THIN * (length(against) + 1)])

palette <- c("#3060c0", "#c05030", "#309050", "#9050b0")
colour_of <- setNames(palette[seq_along(against)], against)

# every power of ten the curves reach, and the line at one, written out rather
# than as 1e+03
decades_over <- function(values) {
  values <- values[is.finite(values) & values > 0]
  10^(floor(log10(min(values))):ceiling(log10(max(values))))
}
plainly <- function(values) formatC(values, format = "fg", drop0trailing = TRUE)

png(output, width = 1400, height = 800, res = 130)
par(mar = c(4.5, 4.5, 3, 1), las = 1)

ylim <- range(c(speedups, 1), na.rm = TRUE)
plot(NA,
  xlim = range(exponents), ylim = ylim, log = "y", xaxt = "n", yaxt = "n",
  xlab = "Dijkstra rank", ylab = sprintf("times faster than %s", name_of(baseline)),
  main = sprintf("speedup over a plain search (%s)", basename(input))
)
abline(v = exponents, col = "#00000010")
# at one the two cost the same. Below it the cells are not paying for
# themselves, which is what the left of the axis should look like: both ends of
# a short query sit in one cell, and there is nothing there to step over.
abline(h = 1, lty = 2, col = "#808080")
for (engine in against) {
  lines(exponents, speedups[, engine], type = "b", pch = 19, col = colour_of[engine])
}
axis(1, at = exponents, labels = parse(text = sprintf("2^%d", exponents)))
ticks <- decades_over(c(speedups, 1))
axis(2, at = ticks, labels = plainly(ticks))

# the curves climb from left to right, so the top left corner is the empty one
best_of <- sapply(against, function(engine) {
  best <- which.max(speedups[, engine])
  sprintf("%s (best %sx at 2^%d)", name_of(engine),
          plainly(signif(speedups[best, engine], 3)), exponents[best])
})
legend("topleft", legend = best_of, col = colour_of[against],
       lty = 1, pch = 19, bty = "n")

# where a curve crosses one is where the preprocessing starts paying, and it is
# worth saying in numbers rather than leaving to the eye
crossings <- sapply(against, function(engine) {
  over <- which(speedups[, engine] >= 1)
  if (length(over) == 0) NA else exponents[min(over)]
})
if (any(!is.na(crossings))) {
  mtext(sprintf("pays for itself from: %s",
                paste(sprintf("%s at 2^%d", name_of(against[!is.na(crossings)]),
                              crossings[!is.na(crossings)]), collapse = ", ")),
        side = 3, line = 0.2, cex = 0.75, col = "#606060")
}
if (length(thin) > 0) {
  mtext(sprintf("thin buckets, under %d samples: 2^%s", THIN,
                paste(thin, collapse = ", 2^")),
        side = 1, line = 3.2, cex = 0.7, col = "#a04000")
}

invisible(dev.off())
cat(sprintf("wrote %s: %d engines against %s, ranks 2^%d to 2^%d\n",
            output, length(against), baseline, min(exponents), max(exponents)))
