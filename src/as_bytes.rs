//TODO: move to lib?
pub trait AsBytes {
    fn as_bytes(&self) -> &[u8];
}

impl AsBytes for &str {
    fn as_bytes(&self) -> &[u8] {
        str::as_bytes(self)
    }
}