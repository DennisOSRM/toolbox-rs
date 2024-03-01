pub struct BitStreamReader<'a> {
    data: &'a [u8],
    current: u8,
    offset_byte: usize,
    offset_bit: u8,
}

impl<'a> TryFrom<&'a [u8]> for BitStreamReader<'a> {
    type Error = &'static str;

    fn try_from(data: &'a [u8]) -> Result<Self, Self::Error> {
        let current = match data.first() {
            None => return Err("Could not be converted"),
            Some(value) => value,
        };

        Ok(Self {
            data,
            current: *current,
            offset_byte: usize::MAX,
            offset_bit: 0x00,
        })
    }
}

impl<'a> Iterator for BitStreamReader<'a> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset_bit == 0x00 {
            self.offset_byte += 1;
            self.offset_bit = 0x01;
            self.current = match self.data.get(self.offset_byte) {
                None => return None,
                Some(value) => *value,
            };
        }

        let bit_set = self.current & self.offset_bit;
        self.offset_bit <<= 1;

        Some(bit_set.min(1))
    }
}

impl<'a> BitStreamReader<'a> {
    pub fn take_n(&mut self, _count: usize) -> u8 {
        unimplemented!();
    }

    pub fn peek_byte(&self) -> u8 {
        unimplemented!()
    }

    pub fn skip_byte(&mut self) {
        unimplemented!()
    }

    pub fn skip_n(&mut self) {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn first_test() {
        unimplemented!()
    }
}
