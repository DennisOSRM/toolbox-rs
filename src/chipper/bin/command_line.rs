use std::ops::RangeInclusive;

const RECURSION_RANGE: RangeInclusive<usize> = 0..=15;

/// Checks whether the number of level is within the expected range of (0, 15].
pub fn recursion_in_range(s: &str) -> Result<usize, String> {
    let recursion: usize = s.parse().map_err(|_| format!("`{}` isn't a number", s))?;
    if RECURSION_RANGE.contains(&recursion) {
        Ok(recursion)
    } else {
        Err(format!(
            "recursion not in range {}-{}",
            RECURSION_RANGE.start(),
            RECURSION_RANGE.end()
        ))
    }
}
