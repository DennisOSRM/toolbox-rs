#!/usr/bin/env Rscript
#
# Draws what the offline store costs against the memory it is given.
#
#   Rscript scripts/budget_plot.R budgets.csv budgets.pdf
#
# The input is what the paged_query example writes with TOOLBOX_SUMMARY, one
# row a budget:
#
#   budget,share,pinned_from,pinned,cache,mld_median,offline_median,p95,slowdown,reads,hits
#
# Three panels on a shared budget axis, because one number does not say it.
# (a) is the median query, (b) the ninety-fifth, and the two are four decades
# apart at the small budgets, so they get a panel each rather than one axis on
# which neither can be read. (c) is how many blocks a query had to read, which
# is what the two above are made of.
#
# Each query panel carries the time on the left and the multiple of the
# in-memory query on the right, so the plot can be read either as a latency or
# as a cost, and the in-memory query itself is the dashed line across it.
#
# The output is vector where the name ends in .pdf, since that is what a paper
# wants, and a raster otherwise.
#
# Base graphics only, as with the plots beside it.

args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) {
  stop("usage: budget_plot.R <budgets.csv> [out.pdf|out.png]")
}
input <- args[1]
output <- if (length(args) >= 2) args[2] else "budgets.pdf"

runs <- read.csv(input, stringsAsFactors = FALSE)
for (column in c("budget", "mld_median", "offline_median", "p95", "reads")) {
  if (!column %in% names(runs)) {
    stop(sprintf("%s has no %s column", input, column))
  }
}
runs <- runs[order(runs$budget), ]
room <- log2(runs$budget)

# the in-memory query the multiples are taken against; it is measured again at
# every budget and barely moves, so one number stands for all of them
reference <- mean(runs$mld_median)
drift <- max(abs(runs$mld_median - reference)) / reference

# times in whatever unit keeps them short, since a paper reads µs at one end of
# this and seconds at the other
in_time <- function(micros) {
  ifelse(micros < 1000, sprintf("%gµs", signif(micros, 3)),
    ifelse(micros < 1e6, sprintf("%gms", signif(micros / 1000, 3)),
      sprintf("%gs", signif(micros / 1e6, 3))
    )
  )
}
# the 1-2-5 decades a reader takes a number off
times_over <- function(values) {
  decades <- 10^(floor(log10(min(values))):ceiling(log10(max(values))))
  ticks <- sort(as.vector(outer(c(1, 2, 5), decades)))
  ticks <- ticks[ticks >= min(values) * 0.85 & ticks <= max(values) * 1.15]
  while (length(ticks) > 9) ticks <- ticks[seq(1, length(ticks), by = 2)]
  ticks
}
as_multiple <- function(values) {
  paste0(formatC(values, format = "fg", big.mark = ",", drop0trailing = TRUE), "×")
}
# the multiples a reader looks for, kept to the span the panel covers
ratios_over <- function(span) {
  decades <- log10(span[2] / span[1])
  wanted <- if (decades > 2) {
    # a wide span reads as a regular sequence and not as whatever round numbers
    # happen to survive being thinned
    sort(as.vector(outer(c(1, 3), 10^(-1:5))))
  } else {
    c(1, 1.25, 1.5, 2, 3, 4, 5, 7.5, 10, 15, 20, 30, 50, 75, 100)
  }
  ticks <- wanted[wanted >= span[1] * 0.99 & wanted <= span[2] * 1.01]
  while (length(ticks) > 8) ticks <- ticks[seq(1, length(ticks), by = 2)]
  ticks
}

# the budgets crowd where they are close together, so they go on two rows
budget_axis <- function(labelled) {
  odd <- seq(1, length(room), by = 2)
  even <- seq(2, length(room), by = 2)
  axis(1, at = room, labels = FALSE, lwd = 0, lwd.ticks = 1)
  if (labelled) {
    axis(1, at = room[odd], labels = sprintf("%g", runs$budget[odd]),
         tick = FALSE, line = -0.5, cex.axis = 1)
    axis(1, at = room[even], labels = sprintf("%g", runs$budget[even]),
         tick = FALSE, line = 0.2, cex.axis = 1)
  }
}

# one panel of query time, with the time on the left and the cost on the right
time_panel <- function(values, colour, label, corner) {
  ticks <- times_over(c(values, reference))
  span <- range(c(ticks, values, reference)) * c(0.88, 1.35)
  plot(NA,
    xlim = range(room) + c(-0.4, 0.4), ylim = span,
    log = "y", xaxt = "n", yaxt = "n", xlab = "", ylab = "", bty = "l"
  )
  abline(h = ticks, col = "#00000012", lwd = 1)
  budget_axis(FALSE)
  axis(2, at = ticks, labels = in_time(ticks), cex.axis = 1)
  mtext(label, side = 2, line = 3.6, las = 0, cex = 0.78)
  # the same panel read as a cost rather than a latency
  right <- ratios_over(span / reference)
  axis(4, at = right * reference, labels = as_multiple(right), cex.axis = 1)
  # far enough out to clear the widest of the tick labels beside it
  mtext("relative to in memory", side = 4,
        line = 1.4 + 0.55 * max(nchar(as_multiple(right))), las = 0, cex = 0.78)
  # the query this is all being compared against
  abline(h = reference, lty = 2, lwd = 1.6, col = "#606060")
  text(max(room) + 0.4, reference, adj = c(1, -0.5), cex = 0.85, col = "#606060",
       labels = sprintf("in memory, %s", in_time(reference)))
  # where the store begins holding whole levels rather than caching blocks
  if ("pinned" %in% names(runs) && any(runs$pinned > 0)) {
    abline(v = log2(min(runs$budget[runs$pinned > 0])), lty = 3, lwd = 1.6,
           col = "#2f8f52")
  }
  lines(room, values, type = "b", pch = 19, lwd = 3.2, cex = 1.15, col = colour)
  mtext(corner, side = 3, line = 0.2, at = min(room) - 0.4, adj = 0, cex = 0.85,
        font = 2)
}

vector_out <- grepl("\\.pdf$", output, ignore.case = TRUE)
if (vector_out) {
  pdf(output, width = 7.2, height = 8.4, pointsize = 10)
} else {
  png(output, width = 7.2, height = 8.4, units = "in", res = 300, pointsize = 10)
}
layout(matrix(c(1, 2, 3), nrow = 3), heights = c(1, 1, 0.95))
par(mar = c(1.6, 5.2, 1.8, 6.2), mgp = c(3, 0.7, 0), las = 1, lwd = 1.1)

time_panel(runs$offline_median, "#20449c", "median query time", "(a)")
if ("pinned" %in% names(runs) && any(runs$pinned > 0)) {
  held <- min(runs$budget[runs$pinned > 0])
  text(log2(held), 10^par("usr")[4], adj = c(1.06, 1.7), cex = 0.85,
       col = "#2f8f52",
       labels = sprintf("levels held outright from %g MiB", held))
}

par(mar = c(1.6, 5.2, 1.8, 6.2))
time_panel(runs$p95, "#a83a20", "95th percentile query time", "(b)")

# and what both are made of
par(mar = c(4.4, 5.2, 1.8, 6.2))
seen <- pmax(runs$reads, 0.05)
counts <- c(0.1, 0.2, 0.5, 1, 2, 5, 10, 20, 50, 100, 200)
counts <- counts[counts >= min(seen) * 0.8 & counts <= max(seen) * 1.3]
plot(NA,
  xlim = range(room) + c(-0.4, 0.4), ylim = range(c(seen, counts)) * c(0.8, 1.3),
  log = "y", xaxt = "n", yaxt = "n", xlab = "", ylab = "", bty = "l"
)
abline(h = counts, col = "#00000012", lwd = 1)
if ("pinned" %in% names(runs) && any(runs$pinned > 0)) {
  abline(v = log2(min(runs$budget[runs$pinned > 0])), lty = 3, lwd = 1.6,
         col = "#2f8f52")
}
lines(room, seen, type = "b", pch = 19, lwd = 3.2, cex = 1.15, col = "#6f3a9c")
budget_axis(TRUE)
axis(2, at = counts, labels = sprintf("%g", counts), cex.axis = 1)
mtext("blocks read per query", side = 2, line = 3.6, las = 0, cex = 0.78)
mtext("(c)", side = 3, line = 0.2, at = min(room) - 0.4, adj = 0, cex = 0.85,
      font = 2)
mtext("memory for the cell tables (MiB)", side = 1, line = 2.9, cex = 0.82)

invisible(dev.off())
slow <- runs$offline_median / reference
cat(sprintf(
  "wrote %s: %d budgets, %g to %g MiB, %.2fx down to %.2fx at the median\n",
  output, nrow(runs), min(runs$budget), max(runs$budget), max(slow), min(slow)
))
cat(sprintf(
  "in memory %.1fus, and no budget measured it more than %.1f%% off that\n",
  reference, 100 * drift
))
