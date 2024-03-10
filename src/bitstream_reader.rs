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

pub struct BitStreamIterator<'a> {
    reader: &'a mut BitStreamReader<'a>,
}

impl<'a> Iterator for BitStreamIterator<'a> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if self.reader.offset_bit == 0x00 {
            self.reader.offset_byte += 1;
            self.reader.offset_bit = 0x01;
            self.reader.current = match self.reader.data.get(self.reader.offset_byte) {
                None => return None,
                Some(value) => *value,
            };
        }

        let bit_set = self.reader.current & self.reader.offset_bit;
        self.reader.offset_bit <<= 1;

        Some(bit_set.min(1))
    }
}

impl<'a> BitStreamReader<'a> {
    pub fn iter(&'a mut self) -> BitStreamIterator {
        BitStreamIterator { reader: self }
    }
}

// #[cfg(test)]
// mod tests {

//     #[test]
//     fn first_test() {
//         unimplemented!()
//     }
// }
