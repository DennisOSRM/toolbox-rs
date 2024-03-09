struct Probability {
    pub upper: u32,
    pub lower: u32,
    pub denominator: u32,
}

#[derive(Default)]
struct Model {}

impl Model {
    pub fn get_probability(&mut self, _character: &u8) -> Probability {
        Probability {
            upper: 55,
            lower: 45,
            denominator: 100,
        }
    }
}

#[derive(Default)]
pub struct RangeCoder {
    model: Model,
}

impl RangeCoder {
    pub fn encode(&mut self, input: &[u8]) {

        let mut low = 0_u32;
        let mut high = 0xFFFF_FFFF_u32;

        let mut pending_bits = 0;

        for c in input {
            let range = high - low + 1;
            let p = self.model.get_probability(c);
            low += (range * p.lower) / p.denominator;
            high = low + (range * p.upper) / p.denominator;

            loop {
                if high < 0x8000_0000_u32 {
                    self.output_bit_plus_pending(false, pending_bits);
                    low <<= 1;
                    high <<= 1;
                    high |= 1;
                } else if low >= 0x8000_0000_u32 {
                    self.output_bit_plus_pending(true, pending_bits);
                    low <<= 1;
                    high <<= 1;
                    high |=1;
                } else if low >= 0x4000_0000_u32 && high < 0x0c00_0000_u32 {
                    pending_bits += 1;
                    low <<= 1;
                    low &= 0x7FFF_FFFF_u32;
                    high <<= 1;
                    high |= 0x8000_0001_u32;
                } else {
                    break;
                }
            }
        }

    }

    fn output_bit_plus_pending(&self, bit: bool, pending_bits: usize) -> usize{
        self.output_bit(bit);
        for _ in 0..pending_bits {
            self.output_bit(!bit);
        }
        0
    }

    fn output_bit(&self, bit: bool) {
        match bit {
            true => { print!("1"); },
            false => { print!("0"); },
        }
    }
}