use crate::math::choose;

/// Implementation of Cover's algorithm to enumerate of sequences of weight w, cf.
/// "Enumerative Source Encoding". Thomas M. Cover. 
/// Appears in IEEE Transactions on Information Theory. Vol 19, Issue: 1, Jan 1973
///
/// The implementation corresponds to Section "Example 2 - Enumeration of Sequences of Weight w".
///
/// # Examples
///
/// ```
/// use toolbox_rs::enumerative_coding::decode;
///
/// // 0th number with 3 bits set in a 64 bit number
/// assert_eq!(decode_u64(3, 0), 0b000_0111);    
/// ```
pub fn decode_u64(mut ones: u64, mut ordinal: u64) -> u64 {
    debug_assert!(ordinal < choose(64, ones));
    let mut result = 0;
    for bit in (0..64).rev() {
        let n_ck = choose(bit, ones);
        if ordinal >= n_ck {
            ordinal -= n_ck;
            result |= 1 << bit;
            ones -= 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use crate::enumerative_source_coding::decode_u64;

    #[test]
    fn paper_examples() {
        // 0th number with 3 bits set
        assert_eq!(decode_u64(3, 0), 0b0000_0111);
        // 1st number with 3 bits set
        assert_eq!(decode_u64(3, 1), 0b0000_1011);
        // 21st number with 3 bits set
        assert_eq!(decode_u64(3, 21), 0b0100_0101);
        // 34th number with 3 bits set
        assert_eq!(decode_u64(3, 34), 0b0111_0000);
        // 41663th (last) number with 3 bits set
        assert_eq!(
            decode_u64(3, 41663),
            0b1110_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000_0000
        );
    }
}
