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
#   ranks time -g graph -i pairs.csv -e bidirectional -o b.csv
#   ranks time -g graph -d levels -i pairs.csv -e mld -o m.csv
#   cat d.csv <(tail -n +2 b.csv) <(tail -n +2 m.csv) > timings.csv
#
# Base graphics only, and no package beyond what R ships with. A plot that
# wants an install first is a plot that does not get looked at, and this one is
# meant to run wherever the numbers were made.

args <- commandArgs(trailingOnly = TRUE)
if (length(args) < 1) {
  stop("usage: rank_plot.R <timings.csv> [out.png] [engine-to-divide-by]")
}
input <- args[1]
output <- if (length(args) >= 2) args[2] else "ranks.png"
# What the lower panel divides by. The engine over the cells by default, that
# being the thing a plain search is usually held against here, but a file may
# hold anything worth comparing -- two ways of unpacking a path, say -- and
# then the one to divide by is whichever the others are being judged against.
denominator <- if (length(args) >= 3) args[3] else "mld"
# what the rows are timings of; only the wording changes with it
measuring <- if (length(args) >= 4) args[4] else "query time"
# What the `nanos` column actually holds. Times are the usual case and are
# drawn in milliseconds; a count -- blocks read for a query, say -- is drawn as
# it stands, since there is no unit to convert it into.
counting <- length(args) >= 5 && args[5] == "count"

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

# milliseconds read better than nanoseconds, and the rank axis is drawn at the
# exponent so the ticks say 2^10 rather than 1024
# A query that read nothing is a real answer and a log axis has nowhere to put
# it, so counts are floored at a half: everything drawn at the bottom of the
# axis read nothing at all.
timings$millis <- if (counting) pmax(timings$nanos, 0.5) else timings$nanos / 1e6
timings$exponent <- round(log2(timings$rank))
engines <- sort(unique(timings$engine))
exponents <- sort(unique(timings$exponent))

# the engine column says which search wrote the row; a legend wants to say
# which search it was, and "dijkstra" does not distinguish the plain search
# from the one that runs from both ends
display <- c(
  dijkstra = "unidirectional", bidirectional = "bidirectional", mld = "mld",
  offline = "offline",
  # the same two again for putting the way back rather than costing it
  "mld-unpack" = "mld, unpacked", "offline-unpack" = "offline, unpacked"
)
name_of <- function(engine) ifelse(engine %in% names(display), display[engine], engine)

# every power of ten the numbers reach, written out rather than as 1e+03: a
# reader who wants to know what a query costs should not have to decode it
decades_over <- function(values) {
  values <- values[is.finite(values) & values > 0]
  10^(floor(log10(min(values))):ceiling(log10(max(values))))
}

# a count reads better in ones and tens than in powers of ten alone
counts_over <- function(values) {
  wanted <- c(0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000)
  wanted[wanted >= min(values) * 0.9 & wanted <= max(values) * 1.1]
}

# The lower panel is a ratio, and a ratio worth reading is usually within a
# factor of a few, where decades give one tick and say nothing. These are the
# multiples a reader looks for, kept to the ones the data reaches, and one is
# always among them because one is where the two engines cost the same.
ratios_over <- function(values) {
  values <- values[is.finite(values) & values > 0]
  wanted <- c(
    0.1, 0.125, 0.15, 0.2, 0.25, 0.33, 0.5, 0.6, 0.7, 0.8, 0.9,
    1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5, 7.5, 10, 15, 20, 30, 50, 100
  )
  kept <- wanted[wanted >= min(values) * 0.98 & wanted <= max(values) * 1.02]
  sort(unique(c(kept, 1)))
}

# 1.5 rather than 1.50, and with the sign a reader expects on a multiple
as_multiple <- function(values) {
  paste0(formatC(values, format = "fg", drop0trailing = TRUE), "\u00d7")
}
plainly <- function(values) formatC(values, format = "fg", drop0trailing = TRUE)

# a bucket holding a handful of samples is drawn like the rest and marked, so
# that a thin tail reads as a thin tail rather than as a result
THIN <- 5
counts <- table(timings$exponent)
thin <- as.integer(names(counts)[counts < THIN * length(engines)])

# enough for a sweep of block sizes with the engine they are measured against
# Okabe and Ito's, which survive the common kinds of colour blindness and stay
# distinct in grey; the same set experiments/paper.R uses, so a paper's figures
# are told apart the same way throughout.
palette <- c(
  "#0072B2", "#D55E00", "#009E73", "#CC79A7",
  "#E69F00", "#56B4E9", "#F0E442", "#000000"
)
colour_of <- setNames(palette[seq_along(engines)], engines)

# A figure that goes into a paper is vector and is drawn at the size it will be
# printed, so that eight point text is eight point text. A `.png` name still
# gives a raster, for a look on a screen.
paper <- !grepl("\\.png$", output, ignore.case = TRUE)
if (paper) {
  pdf(output, width = 7.0, height = 4.6, pointsize = 8, family = "Times",
      useDingbats = FALSE)
  par(mgp = c(1.8, 0.5, 0), tcl = -0.25)
} else {
  png(output, width = 1400, height = 1000, res = 130)
}
# The lower panel is a ratio, and there is no ratio to draw where only one
# engine wrote to the file: it gets the whole device instead of two thirds of
# it and a line of apology.
ratio_panel <- length(engines) > 1 && denominator %in% engines
if (ratio_panel) {
  layout(matrix(c(1, 2), nrow = 2), heights = c(2, 1))
  par(mar = if (paper) c(1.2, 3.6, 1.6, 0.6) else c(1.5, 4.5, 2.5, 1), las = 1)
} else {
  par(mar = if (paper) c(2.6, 3.6, 1.6, 0.6) else c(4.5, 4.5, 2.5, 1), las = 1)
}

# what each engine costs, as a box per bucket
groups <- list()
at <- c()
fill <- c()
width <- 0.8 / length(engines)
for (index in seq_along(exponents)) {
  for (which in seq_along(engines)) {
    rows <- timings$exponent == exponents[index] & timings$engine == engines[which]
    groups[[length(groups) + 1]] <- timings$millis[rows]
    offset <- (which - (length(engines) + 1) / 2) * width
    at <- c(at, exponents[index] + offset)
    fill <- c(fill, colour_of[engines[which]])
  }
}

boxplot(groups,
  at = at, boxwex = width * 0.9, col = fill, log = "y",
  xaxt = "n", yaxt = "n", xlab = "",
  ylab = if (counting) measuring else "milliseconds",
  # A figure in a paper has a caption and does not want a title as well, and
  # certainly not the name of the file it was drawn from. On a screen both are
  # useful for telling one exploratory plot from the next.
  main = if (paper) "" else sprintf("%s by Dijkstra rank (%s)", measuring, basename(input)),
  outcex = 0.3, whisklty = 1, staplewex = 0.5
)
axis(1, at = exponents, labels = parse(text = sprintf("2^%d", exponents)))
ticks <- if (counting) counts_over(timings$millis) else decades_over(timings$millis)
axis(2, at = ticks, labels = if (counting) {
  ifelse(ticks < 1, "none", formatC(ticks, format = "d", big.mark = ","))
} else {
  plainly(ticks)
})
abline(v = exponents[-1] - 0.5, col = "#00000012")
# the searches climb from left to right, so the top left corner is the one
# nothing is drawn in
legend("topleft", legend = name_of(engines), fill = colour_of[engines], bty = "n")
if (length(thin) > 0) {
  mtext(sprintf("thin buckets, under %d samples: 2^%s", THIN,
                paste(thin, collapse = ", 2^")),
        side = 3, line = 0, cex = 0.7, col = "#a04000")
}

# and what one is worth against another, which is the curve to read
if (!ratio_panel) {
  mtext("Dijkstra rank", side = 1, line = 3.2)
  invisible(dev.off())
  cat(sprintf(
    "wrote %s: %d timings, %d engines, ranks 2^%d to 2^%d\n",
    output, nrow(timings), length(engines), min(exponents), max(exponents)
  ))
  quit(status = 0)
}

# the lower panel's title is a pair of engine names and wants the room
par(mar = if (paper) c(2.6, 4.4, 0.6, 0.6) else c(4.5, 4.5, 1, 1))
medians <- sapply(engines, function(engine) {
  sapply(exponents, function(exponent) {
    rows <- timings$exponent == exponent & timings$engine == engine
    if (any(rows)) median(timings$millis[rows]) else NA
  })
})
medians <- matrix(medians, nrow = length(exponents), dimnames = list(NULL, engines))

if (denominator %in% engines && length(engines) > 1) {
  # everything else in the file against the one being judged against. Where
  # that is the search over the cells, the plain unidirectional search is the
  # one a rank axis is defined by but not the yardstick an overlay has to
  # beat: two searches from two ends cost nothing but a second queue, so what
  # the cells are worth is what they beat that by.
  against <- setdiff(engines, denominator)
  ratios <- sapply(against, function(engine) medians[, engine] / medians[, denominator])
  ratios <- matrix(ratios, nrow = length(exponents), dimnames = list(NULL, against))
  ylim <- range(c(ratios, 1), na.rm = TRUE)
  # named after the one engine being divided, where there is only one, so the
  # axis says what is being read rather than "other"
  measured <- if (length(against) == 1) name_of(against) else "other"
  plot(NA,
    xlim = range(exponents), ylim = ylim, log = "y", xaxt = "n", yaxt = "n",
    xlab = "Dijkstra rank",
    ylab = ""
  )
  axis(1, at = exponents, labels = parse(text = sprintf("2^%d", exponents)))
  # placed by hand rather than as ylab, so a long pair of names can be set
  # smaller instead of running off the edge of the device
  side_label <- sprintf("%s / %s", measured, name_of(denominator))
  mtext(side_label, side = 2, line = if (paper) 3.0 else 3.2, las = 0,
        cex = if (nchar(side_label) > 28) 0.72 else 0.9)
  ticks <- ratios_over(c(ratios, 1))
  axis(2, at = ticks, labels = as_multiple(ticks))
  # a line at each tick, so a point can be read across to the axis rather than
  # guessed at between two decades
  abline(h = ticks, col = "#00000014")
  for (engine in against) {
    lines(exponents, ratios[, engine], type = "b", pch = 19, col = colour_of[engine])
  }
  # at one the two cost the same; below it the cells are not paying for
  # themselves, and it should climb as more of them can be stepped over. A
  # curve that does not climb is the thing to look for: every level is sound to
  # step over, so a query that picks a low one gives the same answers and no
  # test of those answers would say a word.
  abline(h = 1, lty = 2, col = "#808080")
  # what each curve is worth at its best goes in the legend rather than beside
  # it, so that nothing is written over the curves. They start low on the left
  # and end high on the right, so the top left corner is the empty one — the
  # bottom right has the line at one running through it.
  # the largest the ratio gets and where, which is the best of a speedup and
  # the worst of a slowdown; either way it is the number to look at
  best_of <- sapply(against, function(engine) {
    best <- which.max(ratios[, engine])
    sprintf("%s / %s (up to %.2f\u00d7 at 2^%d)", name_of(engine), name_of(denominator),
            ratios[best, engine], exponents[best])
  })
  legend(if (paper) "bottomleft" else "topleft", legend = best_of, col = colour_of[against],
         lty = 1, pch = 19, bty = "n")

} else {
  plot.new()
  mtext(sprintf("nothing named %s in the file to divide by", denominator),
        side = 3, line = -3, cex = 0.8, col = "#808080")
}

invisible(dev.off())
cat(sprintf("wrote %s: %d timings, %d engines, ranks 2^%d to 2^%d\n",
            output, nrow(timings), length(engines),
            min(exponents), max(exponents)))
