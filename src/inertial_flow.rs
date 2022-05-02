use std::ops::Index;

pub struct Coefficients([(i32, i32); 4]);
// coefficients for rotation matrix at 0, 90, 180 and 270 degrees
// defined by sin(angle), cosine is shifted by 90 degrees ie one offset

impl Default for Coefficients {
    fn default() -> Self {
        Self::new()
    }
}

impl Coefficients {
    pub fn new() -> Self {
        Coefficients([(0, 1), (1, 0), (1, 1), (-1, 1)])
    }
}

impl Index<usize> for Coefficients {
    type Output = (i32, i32);
    fn index(&self, i: usize) -> &(i32, i32) {
        &self.0[i % self.0.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::Coefficients;

    #[test]
    fn iterate_withwrap() {
        let coefficients = Coefficients::new();

        (0..4).zip(4..8).for_each(|index| {
            assert_eq!(coefficients[index.0], coefficients[index.1]);
        });
    }
}
