#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRegion<const N: usize> {
    bytes: [u8; N],
}

impl<const N: usize> Default for MemoryRegion<N> {
    fn default() -> Self {
        Self { bytes: [0; N] }
    }
}

impl<const N: usize> MemoryRegion<N> {
    pub fn read(&self, offset: usize) -> Option<u8> {
        self.bytes.get(offset).copied()
    }

    pub fn write(&mut self, offset: usize, value: u8) -> bool {
        let Some(byte) = self.bytes.get_mut(offset) else {
            return false;
        };

        *byte = value;
        true
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_in_bounds() {
        let mut memory = MemoryRegion::<4>::default();

        assert!(memory.write(2, 0xAB));
        assert_eq!(memory.read(2), Some(0xAB));
    }

    #[test]
    fn rejects_out_of_bounds_access() {
        let mut memory = MemoryRegion::<4>::default();

        assert!(!memory.write(4, 0xAB));
        assert_eq!(memory.read(4), None);
    }
}
