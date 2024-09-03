use super::{
    meta_date::{ChunkMetaData, MetaData},
    DataFromBytes,
};

#[derive(Debug)]
pub enum ReadMessage {
    StartRead,
    ReadReceive { next_start: u32 },
    ReadFinish,
    StartWrite(MetaData),
    Write(ChunkMetaData),
}

impl DataFromBytes for ReadMessage {
    fn from_data(bytes: &[u8]) -> (Self, &[u8]) {
        match bytes[0] {
            0 => (ReadMessage::StartRead, &bytes[1..]),
            1 => {
                let next_start = u32::from_ne_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                (ReadMessage::ReadReceive { next_start }, &bytes[5..])
            }
            2 => (ReadMessage::ReadFinish, &bytes[1..]),
            3 => {
                let (meta_date, bytes) = MetaData::from_data(&bytes[1..]);
                (ReadMessage::StartWrite(meta_date), bytes)
            }
            4 => {
                let (chunk_meta_date, bytes) = ChunkMetaData::from_data(&bytes[1..]);
                (ReadMessage::Write(chunk_meta_date), bytes)
            }
            _ => {
                unreachable!()
            }
        }
    }
    fn bytes(&self) -> Vec<u8> {
        match self {
            ReadMessage::StartRead => vec![0],
            ReadMessage::ReadReceive { next_start } => {
                let mut bytes = vec![1];
                bytes.extend(next_start.to_ne_bytes());
                bytes
            }
            ReadMessage::ReadFinish => vec![2],
            ReadMessage::StartWrite(meta_date) => {
                let mut bytes = vec![3];
                bytes.extend(meta_date.bytes());
                bytes
            }
            ReadMessage::Write(chunk_meta_date) => {
                let mut bytes = vec![4];
                bytes.extend(chunk_meta_date.bytes());
                bytes
            }
        }
    }
}

pub enum NotifyMessage {
    DataUpdate,
    ReadReady(MetaData),
    WriteReady { mtu: u16 },
    WriteReceive { next_start: u32 },
    WriteFinish,
    Error(String),
}

impl DataFromBytes for NotifyMessage {
    fn from_data(bytes: &[u8]) -> (Self, &[u8]) {
        match bytes[0] {
            0 => (NotifyMessage::WriteFinish, &bytes[1..]),
            1 => (NotifyMessage::DataUpdate, &bytes[1..]),
            2 => {
                let (meta_data, bytes) = MetaData::from_data(&bytes[1..]);
                (NotifyMessage::ReadReady(meta_data), bytes)
            }
            3 => {
                let mtu = u16::from_ne_bytes([bytes[1], bytes[2]]);
                (NotifyMessage::WriteReady { mtu }, &bytes[3..])
            }
            4 => {
                let next_start = u32::from_ne_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                (NotifyMessage::WriteReceive { next_start }, &bytes[5..])
            }
            5 => (
                NotifyMessage::Error(String::from_utf8_lossy(&bytes[1..]).to_string()),
                &[],
            ),
            _ => {
                unreachable!()
            }
        }
    }
    fn bytes(&self) -> Vec<u8> {
        match self {
            NotifyMessage::WriteFinish => vec![0],
            NotifyMessage::DataUpdate => vec![1],
            NotifyMessage::ReadReady(meta_data) => {
                let mut bytes = vec![2];
                bytes.extend(meta_data.bytes());
                bytes
            }
            NotifyMessage::WriteReady { mtu } => {
                let mut bytes = vec![3];
                bytes.extend(mtu.to_ne_bytes());
                bytes
            }
            NotifyMessage::WriteReceive { next_start } => {
                let mut bytes = vec![4];
                bytes.extend(next_start.to_ne_bytes());
                bytes
            }
            NotifyMessage::Error(err) => {
                let mut bytes = vec![5];
                bytes.extend(err.as_bytes());
                bytes
            }
        }
    }
}
