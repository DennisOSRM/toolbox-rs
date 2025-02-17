//! Polyline encoding and decoding utilities
//!
//! This module provides functionality to encode and decode polylines using Google's
//! polyline algorithm. See https://developers.google.com/maps/documentation/utilities/polylinealgorithm
//!
//! Reproduced from the above link to comply with the Apache License, Version 2.0
//!
//! Licensed under the Apache License, Version 2.0 (the "License");
//! you may not use this file except in compliance with the License.
//! You may obtain a copy of the License at
//!
//! http://www.apache.org/licenses/LICENSE-2.0
//!
//! Unless required by applicable law or agreed to in writing, software
//! distributed under the License is distributed on an "AS IS" BASIS,
//! WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//! See the License for the specific language governing permissions and
//! limitations under the License.

/// Decodes an encoded path string into a sequence of coordinates
///
/// # Arguments
/// * `encoded_path` - The encoded polyline string
/// * `precision` - The precision factor (default: 5)
///
/// # Examples
/// ```
/// use toolbox_rs::polyline::decode;
///
/// let encoded = "_p~iF~ps|U_ulLnnqC_mqNvxq`@";
/// let points = decode(encoded, 5);
/// assert_eq!(points.len(), 3);
/// assert!((points[0][0] - 38.5).abs() < 1e-10);
/// ```
pub fn decode(encoded_path: &str, precision: i32) -> Vec<[f64; 2]> {
    let factor = 10f64.powi(precision);
    let len = encoded_path.len();
    let mut path = Vec::with_capacity(len / 2);
    let mut index = 0;
    let mut lat = 0;
    let mut lng = 0;

    while index < len {
        let (result, new_index) = decode_unsigned(encoded_path.as_bytes(), index);
        index = new_index;
        lat += if result & 1 != 0 {
            !(result >> 1)
        } else {
            result >> 1
        };

        let (result, new_index) = decode_unsigned(encoded_path.as_bytes(), index);
        index = new_index;
        lng += if result & 1 != 0 {
            !(result >> 1)
        } else {
            result >> 1
        };

        path.push([lat as f64 / factor, lng as f64 / factor]);
    }

    path
}

/// Encodes an array of coordinates into a polyline string
///
/// # Arguments
/// * `path` - Array of coordinate pairs
/// * `precision` - The precision factor (default: 5)
///
/// # Examples
/// ```
/// use toolbox_rs::polyline::encode;
///
/// let path = vec![[38.5, -120.2], [40.7, -120.95], [43.252, -126.453]];
/// let encoded = encode(&path, 5);
/// assert_eq!(encoded, "_p~iF~ps|U_ulLnnqC_mqNvxq`@");
/// ```
pub fn encode(path: &[[f64; 2]], precision: i32) -> String {
    let factor = 10f64.powi(precision);
    let transform = |point: &[f64; 2]| -> [i32; 2] {
        [
            (point[0] * factor).round() as i32,
            (point[1] * factor).round() as i32,
        ]
    };

    polyline_encode_line(path, transform)
}

#[inline(always)]
fn decode_unsigned(encoded: &[u8], mut index: usize) -> (i32, usize) {
    let mut result = 1;
    let mut shift = 0;

    while let Some(&byte) = encoded.get(index) {
        let b = (byte as i32) - 63 - 1;
        index += 1;
        result += b << shift;
        shift += 5;
        if b < 0x1f {
            break;
        }
    }

    (result, index)
}

fn polyline_encode_line<F>(path: &[[f64; 2]], transform: F) -> String
where
    F: Fn(&[f64; 2]) -> [i32; 2],
{
    // guess a rough estimate of the capacity leading to less reallocations
    let mut result = Vec::with_capacity(path.len() * 4);
    let mut start = [0, 0];

    for point in path {
        let end = transform(point);
        polyline_encode_signed(end[0] - start[0], &mut result);
        polyline_encode_signed(end[1] - start[1], &mut result);
        start = end;
    }

    result.into_iter().collect()
}

fn polyline_encode_signed(value: i32, result: &mut Vec<char>) {
    polyline_encode_unsigned(if value < 0 { !(value << 1) } else { value << 1 }, result);
}

fn polyline_encode_unsigned(mut value: i32, result: &mut Vec<char>) {
    while value >= 0x20 {
        result.push(((0x20 | (value & 0x1f)) + 63) as u8 as char);
        value >>= 5;
    }
    result.push((value + 63) as u8 as char);
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
use std::arch::aarch64::*;
#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
use std::arch::x86_64::*;

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
#[repr(align(32))]
struct AlignedFactors {
    scale: [f64; 4],
    inv_scale: [f64; 4],
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
impl<T: Into<f64>> From<T> for AlignedFactors {
    fn from(factor: T) -> Self {
        let f = factor.into();
        Self {
            scale: [f; 4],
            inv_scale: [1.0 / f; 4],
        }
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
unsafe fn encode_points_simd(points: &[[f64; 2]], precision: i32) -> Vec<i32> {
    let factor = 10f64.powi(precision);
    let aligned_factors = AlignedFactors::from(factor);
    let factors = _mm256_load_pd(aligned_factors.scale.as_ptr());

    let mut result = Vec::with_capacity(points.len() * 2);
    let chunks = points.chunks_exact(2);
    let remainder = chunks.remainder();

    for chunk in chunks {
        let coords = _mm256_loadu_pd(chunk.as_ptr() as *const f64);
        let scaled = _mm256_mul_pd(coords, factors);
        let rounded = _mm256_round_pd(scaled, _MM_FROUND_TO_NEAREST_INT | _MM_FROUND_NO_EXC);

        let mut values = [0i32; 4];
        _mm256_store_pd(values.as_mut_ptr() as *mut f64, rounded);

        result.extend_from_slice(&values);
    }

    // Verarbeite übrige Punkte
    for point in remainder {
        result.push((point[0] * factor).round() as i32);
        result.push((point[1] * factor).round() as i32);
    }

    result
}

// ARM NEON Implementation
#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
unsafe fn encode_points_simd(points: &[[f64; 2]], precision: i32) -> Vec<i32> {
    let factor = 10f64.powi(precision);
    let aligned_factors = AlignedFactors::from(factor);

    let mut result = Vec::with_capacity(points.len() * 2);
    let chunks = points.chunks_exact(2);
    let remainder = chunks.remainder();

    for chunk in chunks {
        // Lade 2 Punkte (4 f64-Werte)
        let coords1 = vld1q_f64(chunk[0].as_ptr());
        let coords2 = vld1q_f64(chunk[1].as_ptr());

        // Multipliziere mit Faktor
        let scaled1 = vmulq_n_f64(coords1, factor);
        let scaled2 = vmulq_n_f64(coords2, factor);

        // round to nearest integer
        let rounded1 = vcvtnq_s64_f64(scaled1);
        let rounded2 = vcvtnq_s64_f64(scaled2);

        // store rounded values
        let mut values1 = [0i32; 2];
        let mut values2 = [0i32; 2];
        vst1q_s32(values1.as_mut_ptr(), rounded1);
        vst1q_s32(values2.as_mut_ptr(), rounded2);

        result.extend_from_slice(&values1);
        result.extend_from_slice(&values2);
    }

    // process remaining points
    for point in remainder {
        result.push(round(point[0] * factor));
        result.push(round(point[1] * factor));
    }

    result
}

pub fn encode_simd(path: &[[f64; 2]], precision: i32) -> String {
    if path.is_empty() {
        return String::new();
    }

    #[cfg(any(
        all(target_arch = "x86_64", target_feature = "avx2"),
        all(target_arch = "aarch64", target_feature = "neon")
    ))]
    {
        let mut result = Vec::new();
        let mut prev = [0, 0];

        unsafe {
            let values = encode_points_simd(path, precision);
            for chunk in values.chunks(2) {
                let delta = [chunk[0] - prev[0], chunk[1] - prev[1]];
                polyline_encode_signed(delta[0], &mut result);
                polyline_encode_signed(delta[1], &mut result);
                prev = [chunk[0], chunk[1]];
            }
        }

        return result.into_iter().collect();
    }

    // fallback to scalar implementation
    encode(path, precision)
}

#[cfg(test)]
mod tests {
    use super::*;

    // test data from Google's polyline algorithm documentation
    const DEFAULT: [[f64; 2]; 3] = [[38.5, -120.2], [40.7, -120.95], [43.252, -126.453]];

    const DEFAULT_ROUNDED: [[f64; 2]; 3] = [[39.0, -120.0], [41.0, -121.0], [43.0, -126.0]];

    const SLASHES: [[f64; 2]; 3] = [[35.6, -82.55], [35.59985, -82.55015], [35.6, -82.55]];

    const ROUNDING: [[f64; 2]; 2] = [[0.0, 0.000006], [0.0, 0.000002]];

    const NEGATIVE: [[f64; 2]; 3] = [
        [36.05322, -112.084004],
        [36.053573, -112.083914],
        [36.053845, -112.083965],
    ];

    #[test]
    fn decode_empty() {
        assert!(decode("", 5).is_empty());
    }

    #[test]
    fn decode_default() {
        let decoded = decode("_p~iF~ps|U_ulLnnqC_mqNvxq`@", 5);
        for (i, point) in DEFAULT.iter().enumerate() {
            assert!((decoded[i][0] - point[0]).abs() < 1e-5);
            assert!((decoded[i][1] - point[1]).abs() < 1e-5);
        }
    }

    #[test]
    fn decode_custom_precision() {
        let decoded = decode("_izlhA~rlgdF_{geC~ywl@_kwzCn`{nI", 6);
        for (i, point) in DEFAULT.iter().enumerate() {
            assert!((decoded[i][0] - point[0]).abs() < 1e-6);
            assert!((decoded[i][1] - point[1]).abs() < 1e-6);
        }
    }

    #[test]
    fn decode_precision_zero() {
        let decoded = decode("mAnFC@CH", 0);
        for (i, point) in DEFAULT_ROUNDED.iter().enumerate() {
            assert!((decoded[i][0] - point[0]).abs() < 1.0);
            assert!((decoded[i][1] - point[1]).abs() < 1.0);
        }
    }

    #[test]
    fn roundtrip() {
        let encoded = "gcneIpgxzRcDnBoBlEHzKjBbHlG`@`IkDxIiKhKoMaLwTwHeIqHuAyGXeB~Ew@fFjAtIzExF";
        let decoded = decode(encoded, 5);
        assert_eq!(encode(&decoded, 5), encoded);
    }

    #[test]
    fn roundtrip_slashes() {
        let encoded = encode(&SLASHES, 5);
        let decoded = decode(&encoded, 5);
        for (i, point) in SLASHES.iter().enumerate() {
            assert!((decoded[i][0] - point[0]).abs() < 1e-5);
            assert!((decoded[i][1] - point[1]).abs() < 1e-5);
        }
    }

    #[test]
    fn encode_empty() {
        assert_eq!(encode(&[], 5), "");
    }

    #[test]
    fn encode_default() {
        assert_eq!(encode(&DEFAULT, 5), "_p~iF~ps|U_ulLnnqC_mqNvxq`@");
    }

    #[test]
    fn encode_rounding() {
        assert_eq!(encode(&ROUNDING, 5), "?A?@");
    }

    #[test]
    fn encode_negative() {
        assert_eq!(encode(&NEGATIVE, 5), "ss`{E~kbkTeAQw@J");
    }

    #[test]
    fn encode_custom_precision() {
        assert_eq!(encode(&DEFAULT, 6), "_izlhA~rlgdF_{geC~ywl@_kwzCn`{nI");
    }

    #[test]
    fn encode_precision_zero() {
        assert_eq!(encode(&DEFAULT, 0), "mAnFC@CH");
    }

    #[test]
    fn encode_negative_values() {
        let point = [[-107.3741825, 0.0]];
        let encoded = encode(&point, 7);
        let decoded = decode(&encoded, 7);
        assert!(decoded[0][0] < 0.0);
    }

    #[test]
    fn encode_decode() {
        let points = vec![[38.5, -120.2], [40.7, -120.95], [43.252, -126.453]];
        let encoded = encode(&points, 5);
        assert_eq!(encoded, "_p~iF~ps|U_ulLnnqC_mqNvxq`@");

        let decoded = decode(&encoded, 5);
        for (i, point) in points.iter().enumerate() {
            assert!((decoded[i][0] - point[0]).abs() < 1e-10);
            assert!((decoded[i][1] - point[1]).abs() < 1e-10);
        }
    }

    #[test]
    fn simd_encode() {
        let points = vec![[38.5, -120.2], [40.7, -120.95], [43.252, -126.453]];
        let encoded = encode_simd(&points, 5);
        assert_eq!(encoded, "_p~iF~ps|U_ulLnnqC_mqNvxq`@");
    }

    #[test]
    fn simd_encode_empty() {
        let points: Vec<[f64; 2]> = vec![];
        let encoded = encode_simd(&points, 5);
        assert_eq!(encoded, "");
    }

    #[test]
    fn simd_encode_single_point() {
        let points = vec![[38.5, -120.2]];
        let encoded = encode_simd(&points, 5);
        let scalar_encoded = encode(&points, 5);
        assert_eq!(encoded, scalar_encoded);
    }
}
