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

pub(crate) mod byte_array {
    pub fn serialize<S, const N: usize>(bytes: &[u8; N], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D, const N: usize>(deserializer: D) -> Result<[u8; N], D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ArrayVisitor<const N: usize>;

        impl<'de, const N: usize> serde::de::Visitor<'de> for ArrayVisitor<N> {
            type Value = [u8; N];

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "an array of {N} bytes")
            }

            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                if v.len() != N {
                    return Err(E::invalid_length(v.len(), &self));
                }
                let mut bytes = [0u8; N];
                bytes.copy_from_slice(v);
                Ok(bytes)
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut bytes = [0u8; N];
                for (i, slot) in bytes.iter_mut().enumerate() {
                    *slot = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(i, &self))?;
                }
                Ok(bytes)
            }
        }

        deserializer.deserialize_bytes(ArrayVisitor::<N>)
    }
}

impl<const N: usize> serde::Serialize for MemoryRegion<N> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.bytes)
    }
}

impl<'de, const N: usize> serde::Deserialize<'de> for MemoryRegion<N> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RegionVisitor<const N: usize>;

        impl<'de, const N: usize> serde::de::Visitor<'de> for RegionVisitor<N> {
            type Value = MemoryRegion<N>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a memory region of {N} bytes")
            }

            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                if v.len() != N {
                    return Err(E::invalid_length(v.len(), &self));
                }
                let mut bytes = [0u8; N];
                bytes.copy_from_slice(v);
                Ok(MemoryRegion { bytes })
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut bytes = [0u8; N];
                for (i, slot) in bytes.iter_mut().enumerate() {
                    *slot = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(i, &self))?;
                }
                Ok(MemoryRegion { bytes })
            }
        }

        deserializer.deserialize_bytes(RegionVisitor::<N>)
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
