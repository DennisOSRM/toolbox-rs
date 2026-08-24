//! How a block is squeezed on its way to disk, and back on its way off it.
//!
//! # A byte a block, not a byte a file
//!
//! Which codec a block was written with is written down with the block, so one
//! file may hold blocks written several ways. That costs a byte a block and
//! buys two things worth more than a byte.
//!
//! A build may choose per block: the coarse levels are a few large blocks
//! where a slow codec costs little and saves much, the finest level is many
//! small ones where the cost per fault is what matters. And a build may change
//! its mind later without rewriting what is already shipped, which for a store
//! that ships in pieces and updates in pieces is the difference between a new
//! release and a new download.
//!
//! [`Stored`] is the codec that does nothing. It is not a placeholder: a block
//! whose entries are already packed to the bit may not be worth compressing at
//! all, and a build that finds so says so with a byte.

use std::io::{Read, Write};

/// What a block was squeezed with.
///
/// The numbers are written into files and may not be reused for anything else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Codec {
    /// written out as it stands
    #[default]
    Stored = 0,
    Deflate = 1,
    Zstd = 2,
    Lz4 = 3,
}

/// A codec that a file names and this build does not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownCodec(pub u8);

impl std::fmt::Display for UnknownCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no codec numbered {}", self.0)
    }
}

impl std::error::Error for UnknownCodec {}

impl Codec {
    /// The byte written down beside a block.
    #[must_use]
    pub fn id(self) -> u8 {
        self as u8
    }

    /// Which codec a byte names.
    ///
    /// # Errors
    ///
    /// Returns the byte when this build has no codec of that number, which is
    /// a file from a later version rather than a file that is wrong.
    pub fn of(id: u8) -> Result<Self, UnknownCodec> {
        match id {
            0 => Ok(Self::Stored),
            1 => Ok(Self::Deflate),
            2 => Ok(Self::Zstd),
            3 => Ok(Self::Lz4),
            other => Err(UnknownCodec(other)),
        }
    }

    /// What to call it in a report.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Deflate => "deflate",
            Self::Zstd => "zstd",
            Self::Lz4 => "lz4",
        }
    }

    /// Squeezes a block.
    ///
    /// `effort` is what the codec makes of it: deflate takes nought to nine
    /// and zstd takes one to twenty-two, and each is held to what it takes.
    /// The others ignore it.
    ///
    /// # Panics
    ///
    /// Panics if the codec fails on a buffer, which for these three means it
    /// could not get the room rather than that the input was wrong.
    #[must_use]
    pub fn encode(self, raw: &[u8], effort: i32) -> Vec<u8> {
        match self {
            Self::Stored => raw.to_vec(),
            Self::Deflate => {
                let mut out = flate2::write::DeflateEncoder::new(
                    Vec::new(),
                    flate2::Compression::new(effort.clamp(0, 9) as u32),
                );
                out.write_all(raw).expect("a buffer does not fail to write");
                out.finish().expect("a buffer does not fail to finish")
            }
            Self::Zstd => {
                zstd::encode_all(raw, effort.clamp(1, 22)).expect("a buffer does not fail to write")
            }
            Self::Lz4 => lz4_flex::compress_prepend_size(raw),
        }
    }

    /// Reads a block back.
    ///
    /// `unpacked` is how large it was before it was squeezed, which the block
    /// map holds. Deflate is told so that the room is asked for once.
    ///
    /// # Errors
    ///
    /// Returns a message where the bytes are not what the codec expects.
    pub fn decode(self, stored: &[u8], unpacked: usize) -> Result<Vec<u8>, String> {
        match self {
            Self::Stored => Ok(stored.to_vec()),
            Self::Deflate => {
                let mut out = Vec::with_capacity(unpacked);
                flate2::read::DeflateDecoder::new(stored)
                    .read_to_end(&mut out)
                    .map_err(|why| format!("deflate: {why}"))?;
                Ok(out)
            }
            Self::Zstd => zstd::decode_all(stored).map_err(|why| format!("zstd: {why}")),
            Self::Lz4 => {
                lz4_flex::decompress_size_prepended(stored).map_err(|why| format!("lz4: {why}"))
            }
        }
    }

    /// Every codec this build knows, for a run that compares them.
    #[must_use]
    pub fn all() -> [Self; 4] {
        [Self::Stored, Self::Deflate, Self::Zstd, Self::Lz4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Something with the shape of a block: runs of small numbers at a fixed
    /// width, which is what packed distances look like.
    fn blockish(len: usize) -> Vec<u8> {
        (0..len)
            .map(|at| ((at * 7 + at / 13) % 251) as u8)
            .collect()
    }

    #[test]
    fn every_codec_gives_back_what_it_was_given() {
        for raw in [Vec::new(), blockish(1), blockish(1000), blockish(100_000)] {
            for codec in Codec::all() {
                for effort in [1, 9] {
                    let stored = codec.encode(&raw, effort);
                    let read = codec
                        .decode(&stored, raw.len())
                        .unwrap_or_else(|why| panic!("{} at {effort}: {why}", codec.name()));
                    assert_eq!(read, raw, "{} at effort {effort}", codec.name());
                }
            }
        }
    }

    #[test]
    fn a_codec_is_named_by_the_byte_it_writes() {
        for codec in Codec::all() {
            assert_eq!(Codec::of(codec.id()), Ok(codec));
        }
    }

    /// A file naming a codec this build does not have is a file from later,
    /// and says so rather than being read as something else.
    #[test]
    fn a_codec_this_build_does_not_know_is_refused() {
        assert_eq!(Codec::of(200), Err(UnknownCodec(200)));
        assert_eq!(
            Codec::of(200).unwrap_err().to_string(),
            "no codec numbered 200"
        );
    }

    #[test]
    fn the_ones_that_squeeze_do_squeeze() {
        let raw = blockish(100_000);
        for codec in [Codec::Deflate, Codec::Zstd, Codec::Lz4] {
            let stored = codec.encode(&raw, 3);
            assert!(stored.len() < raw.len(), "{} made it larger", codec.name());
        }
        assert_eq!(Codec::Stored.encode(&raw, 3).len(), raw.len());
    }

    #[test]
    fn rubbish_is_refused_rather_than_read() {
        let nonsense = vec![0xFF_u8; 64];
        // stored takes anything, being a copy; the rest have a shape to check
        for codec in [Codec::Deflate, Codec::Zstd, Codec::Lz4] {
            assert!(
                codec.decode(&nonsense, 1000).is_err(),
                "{} read rubbish",
                codec.name()
            );
        }
    }
}
