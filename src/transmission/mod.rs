use std::sync::{Arc, Condvar};

use esp32_nimble::{utilities::mutex::Mutex, uuid128, NimbleProperties};
use futures::{channel::mpsc, executor::ThreadPool, task::SpawnExt, StreamExt};
use meta_date::{ChunkMetaData, MetaData};
use msg::{NotifyMessage, ReadMessage};
use rand::random;
pub mod meta_date;
pub mod msg;

trait DataFromBytes
where
    Self: Sized,
{
    fn from_data(value: &[u8]) -> (Self, &[u8]);
    fn bytes(&self) -> Vec<u8>;
}

#[derive(Debug, Clone)]
pub enum State {
    Reading,
    Writing,
}

#[derive(Clone)]
pub struct Transmission {
    pub data: Arc<Mutex<Vec<u8>>>,
    pub service: Arc<Mutex<esp32_nimble::BLEService>>,
    pub characteristic: Arc<Mutex<esp32_nimble::BLECharacteristic>>,
    pub state: Arc<std::sync::Mutex<Option<State>>>,
    pub condvar: Arc<Condvar>,
    pub pool: ThreadPool,
}

impl Transmission {
    pub fn new(
        service: Arc<Mutex<esp32_nimble::BLEService>>,
        data: Arc<Mutex<Vec<u8>>>,
        pool: ThreadPool,
    ) -> Self {
        let characteristic = service.lock().create_characteristic(
            uuid128!("ae0e7bca-a1bb-9533-756a-f3546bad65d6"),
            NimbleProperties::NOTIFY | NimbleProperties::READ | NimbleProperties::WRITE,
        );
        characteristic.lock().create_2904_descriptor();
        Self {
            data,
            service,
            characteristic,
            state: Arc::new(std::sync::Mutex::new(None)),
            condvar: Arc::new(Condvar::new()),
            pool,
        }
    }

    pub fn init(&self) {
        let transmission = self.clone();
        let transmission2 = self.clone();

        let start = Arc::new(Mutex::new(0));
        let start2 = start.clone();

        let read_meta_data = Arc::new(Mutex::new(None));
        let read_meta_data2 = read_meta_data.clone();

        let write_meta_data = Arc::new(Mutex::new(None));

        let (mut tx, mut rx) = mpsc::channel::<Vec<u8>>(10);
        let write_mtu = Arc::new(Mutex::new(0));
        let write_mtu2 = write_mtu.clone();

        self.pool
            .spawn(async move {
                while let Some(value) = rx.next().await {
                    let (message, recv_data) = ReadMessage::from_data(&value);
                    #[cfg(debug_assertions)]
                    log::info!("read message: {:?}", message);
                    match message {
                        ReadMessage::StartRead => {
                            let id = random::<u32>();
                            transmission.state.lock().unwrap().replace(State::Reading);
                            transmission.condvar.notify_one();

                            let meta_data = MetaData {
                                id,
                                total_size: transmission.data.lock().len() as u32,
                            };

                            read_meta_data.lock().replace(meta_data.clone());
                            transmission
                                .characteristic
                                .lock()
                                .set_value(&NotifyMessage::ReadReady(meta_data).bytes())
                                .notify();
                            #[cfg(debug_assertions)]
                            log::info!("发送通知读取");
                            *start.lock() = 0;
                            #[cfg(debug_assertions)]
                            log::info!("设置start为0");
                        }
                        ReadMessage::ReadReceive { next_start } => {
                            *start.lock() = next_start;
                        }
                        ReadMessage::ReadFinish => {
                            transmission.state.lock().unwrap().take();
                            transmission.condvar.notify_one();
                        }
                        ReadMessage::StartWrite(meta_data) => {
                            write_meta_data.lock().replace(meta_data);
                            *transmission.data.lock() = vec![];
                            #[cfg(debug_assertions)]
                            log::warn!("替换meta_data");
                            transmission
                                .characteristic
                                .lock()
                                .set_value(
                                    &NotifyMessage::WriteReady {
                                        mtu: *write_mtu.lock(),
                                    }
                                    .bytes(),
                                )
                                .notify();
                            #[cfg(debug_assertions)]
                            log::warn!("发送通知");
                            transmission.state.lock().unwrap().replace(State::Writing);
                            transmission.condvar.notify_one();
                        }
                        ReadMessage::Write(chunk_meta_data) => {
                            let state = transmission.state.lock().unwrap().clone();
                            if let Some(state) = state {
                                if matches!(state, State::Writing) {
                                    let write_meta_data = write_meta_data.lock().clone();
                                    if let Some(write_meta_data) = write_meta_data {
                                        if write_meta_data.id == chunk_meta_data.id {
                                            let mut data = transmission.data.lock();

                                            let next_start =
                                                chunk_meta_data.start + chunk_meta_data.chunk_size;

                                            data.extend(recv_data);

                                            if next_start < write_meta_data.total_size {
                                                transmission
                                                    .characteristic
                                                    .lock()
                                                    .set_value(
                                                        &NotifyMessage::WriteReceive { next_start }
                                                            .bytes(),
                                                    )
                                                    .notify();
                                            } else {
                                                #[cfg(debug_assertions)]
                                                log::warn!("写入完成，数据长度：{}", data.len());
                                                transmission
                                                    .characteristic
                                                    .lock()
                                                    .set_value(&NotifyMessage::WriteFinish.bytes())
                                                    .notify();
                                            }
                                        }
                                    }
                                }
                            }
                            // todo 错误处理
                            // args.reject();
                        }
                    }
                }
            })
            .unwrap();

        self.characteristic
            .lock()
            .on_write(move |args| {
                let value = args.recv_data();
                *write_mtu2.lock() = args.desc().mtu();
                if tx.try_send(value.to_vec()).is_err() {
                    #[cfg(debug_assertions)]
                    log::warn!("发送失败");
                    args.reject();
                }
            })
            .on_read(move |attr, desc| {
                let state = transmission2.state.lock().unwrap().clone();

                if let Some(state) = state {
                    if matches!(state, State::Reading) {
                        let mtu = desc.mtu();
                        let meta_data = read_meta_data2.lock().clone();
                        if let Some(meta_data) = meta_data {
                            let start = *start2.lock();
                            if start < meta_data.total_size {
                                let chunk_meta = ChunkMetaData {
                                    id: meta_data.id,
                                    start,
                                    chunk_size: (mtu as u32 - 12).min(meta_data.total_size - start),
                                };
                                let mut chunk_meta_bytes = chunk_meta.bytes();
                                let data = transmission2.data.lock();
                                let data =
                                    &data[start as usize..(start + chunk_meta.chunk_size) as usize];
                                chunk_meta_bytes.extend(data);
                                attr.set_value(&chunk_meta_bytes);
                                return;
                            }
                        }
                    }
                }
                attr.set_value(&[]);
            });
    }
}
