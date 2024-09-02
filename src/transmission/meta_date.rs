use super::DataFromBytes;

#[derive(Debug, Clone)]
pub struct ChunkMetaData {
    pub id: u32,
    pub start: u32,
    pub chunk_size: u32,
}

impl DataFromBytes for ChunkMetaData {
    fn from_data(value: &[u8]) -> (Self, &[u8]) {
        let meta_date = &value[0..12];
        let chunks = meta_date.chunks(4);
        let mut res = Self {
            id: 0,
            start: 0,
            chunk_size: 0,
        };
        for (i, chunk) in chunks.enumerate() {
            let ptr = chunk.as_ptr() as *const [u8; 4];
            let value = u32::from_ne_bytes(unsafe { std::ptr::read(ptr) });
            match i {
                0 => res.id = value,
                1 => res.start = value,
                2 => res.chunk_size = value,
                _ => {}
            }
        }
        (res, &value[12..])
    }
    fn bytes(&self) -> Vec<u8> {
        let mut data = vec![];
        data.extend(self.id.to_ne_bytes());
        data.extend(self.start.to_ne_bytes());
        data.extend(self.chunk_size.to_ne_bytes());
        data
    }
}

#[derive(Debug, Clone)]
pub struct MetaData {
    pub id: u32,
    pub total_size: u32,
}

impl DataFromBytes for MetaData {
    fn from_data(value: &[u8]) -> (Self, &[u8]) {
        let meta_date = &value[0..8];
        let chunks = meta_date.chunks(4);
        let mut res = Self {
            id: 0,
            total_size: 0,
        };
        for (i, chunk) in chunks.enumerate() {
            let ptr = chunk.as_ptr() as *const [u8; 4];
            let value = u32::from_ne_bytes(unsafe { std::ptr::read(ptr) });
            match i {
                0 => res.id = value,
                1 => res.total_size = value,
                _ => {}
            }
        }
        (res, &value[8..])
    }

    fn bytes(&self) -> Vec<u8> {
        let mut data = vec![];
        data.extend(self.id.to_ne_bytes());
        data.extend(self.total_size.to_ne_bytes());
        data
    }
}
