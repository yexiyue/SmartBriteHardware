use anyhow::Result;
use esp32_nimble::{
    utilities::{mutex::Mutex, BleUuid},
    NimbleProperties,
};
use futures::{channel::mpsc, executor::ThreadPool, task::SpawnExt, StreamExt};
use meta_date::{ChunkMetaData, MetaData};
use msg::{NotifyMessage, ReadMessage};
use rand::random;
use std::sync::{Arc, Condvar};
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
    pub characteristic: Arc<Mutex<esp32_nimble::BLECharacteristic>>,
    pub state: Arc<std::sync::Mutex<Option<State>>>,
    pub condvar: Arc<Condvar>,
    pub pool: ThreadPool,
}

impl Transmission {
    pub fn new(
        service: Arc<Mutex<esp32_nimble::BLEService>>,
        uuid: BleUuid,
        pool: ThreadPool,
    ) -> Self {
        let characteristic = service.lock().create_characteristic(
            uuid,
            NimbleProperties::NOTIFY | NimbleProperties::READ | NimbleProperties::WRITE,
        );
        characteristic.lock().create_2904_descriptor();
        Self {
            data: Arc::new(Mutex::new(vec![])),
            characteristic,
            state: Arc::new(std::sync::Mutex::new(None)),
            condvar: Arc::new(Condvar::new()),
            pool,
        }
    }

    pub fn init<F>(&self, mut on_write_finish: Option<F>)
    where
        F: FnMut(Vec<u8>, &Transmission) -> Result<(), anyhow::Error> + Send + Sync + 'static,
    {
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

                                                let data_clone = data.clone();
                                                drop(data);
                                                // 写入完成重置状态
                                                transmission.state.lock().unwrap().take();
                                                transmission.condvar.notify_one();

                                                transmission
                                                    .characteristic
                                                    .lock()
                                                    .set_value(&NotifyMessage::WriteFinish.bytes())
                                                    .notify();

                                                // 写入成功回调函数
                                                if let Some(on_write) = on_write_finish.as_mut() {
                                                    match on_write(data_clone, &transmission) {
                                                        Ok(_) => {}
                                                        Err(e) => {
                                                            transmission
                                                                .characteristic
                                                                .lock()
                                                                .set_value(
                                                                    &NotifyMessage::Error(
                                                                        e.to_string(),
                                                                    )
                                                                    .bytes(),
                                                                )
                                                                .notify();
                                                        }
                                                    }
                                                }
                                            }
                                            continue;
                                        }
                                    }
                                }
                            }
                            // 发送错误信息
                            transmission
                                .characteristic
                                .lock()
                                .set_value(&NotifyMessage::Error("写入失败".into()).bytes())
                                .notify();
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

    pub fn get_value(&self) -> Result<Vec<u8>> {
        let mut state = self.state.lock().unwrap();
        // 如果正在写入，则等待写入完成再读取数据
        while let Some(_) = &*state {
            state = self.condvar.wait(state).unwrap();
        }
        Ok(self.data.lock().clone())
    }

    pub fn set_value(&self, value: Vec<u8>) -> Result<()> {
        let mut state = self.state.lock().unwrap();

        while let Some(_) = &*state {
            state = self.condvar.wait(state).unwrap();
        }
        *self.data.lock() = value;
        self.notify_update();
        Ok(())
    }

    pub fn notify_update(&self) {
        self.characteristic
            .lock()
            .set_value(&NotifyMessage::DataUpdate.bytes())
            .notify();
    }
}
