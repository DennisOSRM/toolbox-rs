#!/usr/bin/env Rscript
#
# Draws what the offline store costs against the memory it is given.
#
#   Rscript scripts/budget_plot.R budgets.csv budgets.png
#
# The input is what the paged_query example writes with TOOLBOX_SUMMARY, one
# row a budget:
#
#   budget,share,pinned_from,pinned,cache,mld_median,offline_median,p95,slowdown,reads,hits
#
# Three panels on a shared budget axis, because one number does not say it.
# The median and the ninety-fifth are four decades apart at the small budgets,
# so they get a panel each rather than one axis on which neither can be read.
# The third is how many blocks a query had to read, which is what the two
# above are made of.
#
# Base graphics only, as with the plots beside it.

args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) {
  stop("usage: budget_plot.R <budgets.csv> [out.png]")
}
input <- args[1]
output <- if (length(args) >= 2) args[2] else "budgets.png"

runs <- read.csv(input, stringsAsFactors = FALSE)
for (column in c("budget", "mld_median", "offline_median", "reads")) {
  if (!column %in% names(runs)) {
    stop(sprintf("%s has no %s column", input, column))
  }
}
runs <- runs[order(runs$budget), ]
room <- log2(runs$budget)

# the multiples a reader looks for, kept to the ones the data reaches, with one
# always among them because one is where the two engines cost the same
ratios_over <- function(values) {
  values <- values[is.finite(values) & values > 0]
  wanted <- c(
    1, 1.25, 1.5, 2, 2.5, 3, 4, 5, 7.5, 10, 15, 20, 30, 50, 75,
    100, 200, 300, 500, 1000, 2000, 3000, 5000, 10000, 20000, 50000
  )
  ticks <- sort(unique(c(wanted[wanted >= min(values) * 0.9 & wanted <= max(values) * 1.1], 1)))
  # over four decades there are more of these than fit beside an axis, so thin
  # them until they do, always keeping the one at the bottom
  while (length(ticks) > 9) ticks <- ticks[seq(1, length(ticks), by = 2)]
  ticks
}
# the budgets crowd where they are close together, so they go on two rows
budget_axis <- function() {
  odd <- seq(1, length(room), by = 2)
  even <- seq(2, length(room), by = 2)
  axis(1, at = room, labels = FALSE)
  axis(1, at = room[odd], labels = sprintf("%g", runs$budget[odd]),
       tick = FALSE, line = -0.6, cex.axis = 0.85)
  axis(1, at = room[even], labels = sprintf("%g", runs$budget[even]),
       tick = FALSE, line = 0.1, cex.axis = 0.85)
}
as_multiple <- function(values) {
  paste0(formatC(values, format = "fg", big.mark = ",", drop0trailing = TRUE), "×")
}

# one panel of slowdown, with ticks a reader can take a number off
slowdown_panel <- function(values, colour, label, title) {
  ticks <- ratios_over(c(values, 1))
  # room over the highest point and under the lowest, so the labels beside them
  # are not cut off by the frame
  span <- range(c(ticks, values))
  plot(NA,
    xlim = range(room) + c(-0.35, 0.35), ylim = span * c(0.92, 1.3),
    log = "y", xaxt = "n", yaxt = "n", xlab = "", ylab = label, main = title
  )
  budget_axis()
  axis(2, at = ticks, labels = as_multiple(ticks))
  abline(h = ticks, col = "#00000014")
  # at one the two engines cost the same, which is the line to aim at
  abline(h = 1, lty = 2, col = "#808080")
  # where the store begins holding whole levels rather than caching blocks
  if ("pinned" %in% names(runs)) {
    holding <- runs$budget[runs$pinned > 0]
    if (length(holding) > 0) abline(v = log2(min(holding)), lty = 3, col = "#309050")
  }
  lines(room, values, type = "b", pch = 19, lwd = 2, col = colour)
  # the last label would run off the frame above its point, so it goes left
  sides <- c(rep(3, length(room) - 1), 2)
  text(room, values, labels = as_multiple(signif(values, 3)),
       pos = sides, offset = 0.55, cex = 0.65, col = colour)
}

png(output, width = 1400, height = 1300, res = 130)
layout(matrix(c(1, 2, 3), nrow = 3), heights = c(1.25, 1, 0.9))
par(mar = c(2, 6.5, 2.5, 1), las = 1)

slowdown_panel(
  runs$offline_median / runs$mld_median, "#3060c0",
  "median, offline / in memory",
  "what the offline store costs for the memory it is given"
)
if ("pinned" %in% names(runs) && any(runs$pinned > 0)) {
  held <- min(runs$budget[runs$pinned > 0])
  mtext(sprintf("levels held outright from %g MiB", held),
        side = 3, line = -1.3, at = log2(held), adj = 1.05,
        cex = 0.72, col = "#309050")
}

par(mar = c(2, 6.5, 1, 1))
slowdown_panel(
  runs$p95 / runs$mld_median, "#c05030",
  "95th, offline / in memory", ""
)

# and what both are made of
par(mar = c(4.2, 6.5, 1, 1))
seen <- pmax(runs$reads, 0.1)
plot(room, seen,
  type = "b", pch = 19, lwd = 2, col = "#9050b0", log = "y",
  xlim = range(room) + c(-0.35, 0.35), ylim = range(seen) * c(0.8, 1.25),
  xaxt = "n", yaxt = "n", xlab = "memory for the tables, MiB",
  ylab = "blocks read a query"
)
budget_axis()
counts <- c(0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100, 200)
counts <- counts[counts >= min(seen) * 0.8 & counts <= max(seen) * 1.25]
axis(2, at = counts, labels = sprintf("%g", counts))
abline(h = counts, col = "#00000014")

invisible(dev.off())
slow <- runs$offline_median / runs$mld_median
cat(sprintf(
  "wrote %s: %d budgets, %g to %g MiB, %.2fx down to %.2fx at the median\n",
  output, nrow(runs), min(runs$budget), max(runs$budget), max(slow), min(slow)
))
