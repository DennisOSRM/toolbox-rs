use crate::bitstream_reader::BitStreamReader;

struct Probability {
    pub upper: u32,
    pub lower: u32,
    pub denominator: u32,
    pub high: u32,
    pub low: u32,
    pub count: u32,
}

#[derive(Default)]
struct Model {}

impl Model {
    pub fn get_probability(&mut self, _character: &u8) -> Probability {
        // TODO: value needs to depend on input character
        todo!()
    }

    pub fn get_count(&self) -> u32 {
        todo!()
    }

    fn get_character(&self, _count: u32) -> (Probability, u16) {
        todo!()
    }
}

#[derive(Default)]
pub struct RangeCoder {
    model: Model,
}

impl RangeCoder {
    pub fn encode(&mut self, input: &[u8]) {
        // TODO: output should be generic BitStreamWriter
        let mut low = 0_u32;
        let mut high = 0xFFFF_FFFF_u32;

        let mut pending_bits = 0;

        input.iter().for_each(|c| {
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
                    high |= 1;
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
        });
    }

    pub fn decode<'a>(&mut self, input: &'a mut BitStreamReader<'a>) {
        // TODO: output should be generic ByteStreamWriter
        let mut low = 0;
        let mut high = 0xFFFF_FFFF_u32;
        let mut value = 0_u32;

        let mut input_iter = input.iter();

        (0..32).for_each(|_| {
            // preload the value
            value <<= 1;
            if let Some(bit) = input_iter.next() {
                assert!(bit <= 1);
                value += bit as u32;
            }
        });

        loop {
            let range = high - low + 1;
            let count = ((value - low + 1) * self.model.get_count() - 1) / range;
            let (p, character) = self.model.get_character(count);
            assert!(character <= 256);
            if character == 256 {
                // EOF
                break;
            }
            self.output_byte(character as u8);
            high = low + (range * p.high) / p.count - 1;
            low += (range * p.low) / p.count;
            loop {
                if low >= 0x8000_0000_u32 || high < 0x8000_0000_u32 {
                    low <<= 1;
                    high <<= 1;
                    high |= 1;
                    value <<= 1;
                    value += input_iter.next().unwrap() as u32;
                    // TODO: the above line expects a well-formed stream. Real-world requires more error handling
                } else if low >= 0x4000_0000_u32 && high < 0x0c00_0000_u32 {
                    low <<= 1;
                    low &= 0x7FFF_FFFF_u32;
                    high <<= 1;
                    high |= 0x8000_0001_u32;
                    value -= 0x4000_0000_u32;
                    value <<= 1;
                    value += input_iter.next().unwrap() as u32;
                    // TODO: the above line expects a well-formed stream. Real-world requires more error handling
                } else {
                    break;
                }
            }
        }
    }

    fn output_bit_plus_pending(&self, bit: bool, pending_bits: usize) -> usize {
        self.output_bit(bit);
        for _ in 0..pending_bits {
            self.output_bit(!bit);
        }
        0
    }

    fn output_bit(&self, bit: bool) {
        match bit {
            true => {
                print!("1");
            }
            false => {
                print!("0");
            }
        }
    }

    fn output_byte(&self, byte: u8) {
        print!("[{}]", byte as u32)
    }
}
