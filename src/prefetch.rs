//! Asking for a line before it is wanted, and deciding whether it is worth it.
//!
//! A hint is not a load. It cannot fault, it cannot change a result, and on an
//! architecture without one it is nothing at all. What it can do is waste an
//! instruction, which is why [`worth_it`] exists: below the last level cache a
//! search is not missing, and the hints are then a cost with nothing to show.

use std::sync::OnceLock;

/// Ask for a line to be brought into the first level of cache.
#[inline(always)]
pub fn hint<T>(at: *const T) {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: the hint takes any address at all, mapped or not, and has no
    // effect beyond the cache.
    unsafe {
        std::arch::x86_64::_mm_prefetch::<{ std::arch::x86_64::_MM_HINT_T0 }>(at.cast());
    }
    // the aarch64 intrinsic is unstable, so the instruction is written out:
    // preload, first level, keep
    #[cfg(target_arch = "aarch64")]
    // SAFETY: as above. prfm cannot fault and cannot change a result.
    unsafe {
        std::arch::asm!(
            "prfm pldl1keep, [{at}]",
            at = in(reg) at,
            options(nostack, preserves_flags)
        );
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    let _ = at;
}

/// The last level cache, in bytes, asked of the machine once.
///
/// `TOOLBOX_LAST_LEVEL_CACHE` overrides, which is how a threshold is swept. A
/// machine that will not say is taken to have eight mebibytes, so that the
/// decision below still has something to compare against.
pub fn last_level_cache() -> usize {
    static ASKED: OnceLock<usize> = OnceLock::new();
    *ASKED.get_or_init(|| {
        if let Some(given) = std::env::var("TOOLBOX_LAST_LEVEL_CACHE")
            .ok()
            .and_then(|given| given.parse().ok())
        {
            return given;
        }
        #[cfg(target_os = "macos")]
        {
            let mut bytes: u64 = 0;
            let mut width = std::mem::size_of::<u64>();
            // SAFETY: the name is a nul terminated literal, and a u64 is written
            // into a u64 whose width is what is handed over.
            let asked = unsafe {
                libc::sysctlbyname(
                    c"hw.l3cachesize".as_ptr().cast(),
                    std::ptr::from_mut(&mut bytes).cast(),
                    &mut width,
                    std::ptr::null_mut(),
                    0,
                )
            };
            if asked == 0 && bytes > 0 {
                return usize::try_from(bytes).unwrap_or(8 << 20);
            }
        }
        #[cfg(target_os = "linux")]
        for index in 0..8 {
            let at = format!("/sys/devices/system/cpu/cpu0/cache/index{index}");
            if let (Ok(level), Ok(size)) = (
                std::fs::read_to_string(format!("{at}/level")),
                std::fs::read_to_string(format!("{at}/size")),
            ) && level.trim() == "3"
                && let Some(kibibytes) = size.trim().strip_suffix('K')
                && let Ok(kibibytes) = kibibytes.parse::<usize>()
            {
                return kibibytes * 1024;
            }
        }
        8 << 20
    })
}

/// Whether a working set of this many bytes is worth prefetching for.
///
/// The whole of it has to be missing for a hint to buy anything. A search that
/// fits in the last level cache is not waiting on memory, and the hints are
/// then instructions and nothing else.
#[must_use]
pub fn worth_it(working_set: usize) -> bool {
    working_set > last_level_cache()
}

#[cfg(test)]
mod tests {
    use super::{hint, last_level_cache, worth_it};

    #[test]
    fn a_hint_leaves_what_it_asks_for_alone() {
        let numbers = [1_u64, 2, 3, 4];
        hint(std::ptr::from_ref(&numbers[3]));
        assert_eq!(numbers, [1, 2, 3, 4]);
    }

    #[test]
    fn the_machine_names_a_last_level_cache() {
        let bytes = last_level_cache();
        assert!(bytes > 0);
        // asking twice is asking once, so the answer cannot drift
        assert_eq!(bytes, last_level_cache());
    }

    #[test]
    fn only_a_working_set_past_the_cache_is_worth_it() {
        assert!(!worth_it(0));
        assert!(!worth_it(last_level_cache()));
        assert!(worth_it(last_level_cache() + 1));
    }
}
