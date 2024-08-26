use crate::{
    store::Scene,
    timer::{TimerEvent, TimerEventSender},
};
use anyhow::Result;
use esp32_nimble::{
    utilities::mutex::Mutex, uuid128, BLEAdvertisementData, BLEDevice, NimbleProperties,
};
use serde::{Deserialize, Serialize};

use futures::channel::mpsc::Sender;
use std::{sync::Arc, time::Duration};

pub enum LightEvent {
    Close,
    Open,
    SetScene(Scene),
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LightControl {
    Close,
    Open,
    Reset,
}

impl From<&[u8]> for LightControl {
    fn from(data: &[u8]) -> Self {
        match data {
            b"close" => LightControl::Close,
            b"open" => LightControl::Open,
            b"reset" => LightControl::Reset,
            _ => panic!("invalid control"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LightEventSender {
    event_tx: Sender<LightEvent>,
}

impl LightEventSender {
    pub fn new(event_tx: Sender<LightEvent>) -> Self {
        LightEventSender { event_tx }
    }
    pub fn close(&mut self) -> Result<()> {
        Ok(self.event_tx.try_send(LightEvent::Close)?)
    }
    pub fn open(&mut self) -> Result<()> {
        Ok(self.event_tx.try_send(LightEvent::Open)?)
    }
    pub fn set_scene(&mut self, scene: Scene) -> Result<()> {
        Ok(self.event_tx.try_send(LightEvent::SetScene(scene))?)
    }
    pub fn reset(&mut self) -> Result<()> {
        Ok(self.event_tx.try_send(LightEvent::Reset)?)
    }
}

#[derive(Debug, Clone)]
pub enum LightState {
    Opened,
    Closed,
}

impl Into<&'static [u8]> for LightState {
    fn into(self) -> &'static [u8] {
        match self {
            LightState::Opened => b"opened",
            LightState::Closed => b"closed",
        }
    }
}

impl From<&[u8]> for LightState {
    fn from(value: &[u8]) -> Self {
        match value {
            b"opened" => LightState::Opened,
            b"closed" => LightState::Closed,
            _ => panic!("invalid state"),
        }
    }
}

#[derive(Clone)]
pub struct BleControl {
    pub scene_characteristic: Arc<Mutex<esp32_nimble::BLECharacteristic>>,
    pub control_characteristic: Arc<Mutex<esp32_nimble::BLECharacteristic>>,
    pub state_characteristic: Arc<Mutex<esp32_nimble::BLECharacteristic>>,
    pub time_task_characteristic: Arc<Mutex<esp32_nimble::BLECharacteristic>>,
}

impl BleControl {
    pub fn new(light_sender: LightEventSender, mut time_sender: TimerEventSender) -> Result<Self> {
        // 获取BLE设备实例
        let device = BLEDevice::take();

        // 获取并配置BLE的广告实例
        let advertising = device.get_advertising();

        // 获取并配置BLE的服务实例。
        let server = device.get_server();

        // 配置BLE连接时的回调函数
        server.on_connect(|server, desc| {
            log::info!("on_connect: {:#?}", desc);

            server
                .update_conn_params(desc.conn_handle(), 24, 48, 0, 60)
                .unwrap();
            if server.connected_count() < (esp_idf_svc::sys::CONFIG_BT_NIMBLE_MAX_CONNECTIONS as _)
            {
                advertising.lock().start().unwrap();
            }
        });

        // 配置BLE断开连接时的回调函数
        server.on_disconnect(|desc, reason| {
            log::warn!("on_disconnect: {:#?}, reason: {:#?}", desc, reason)
        });

        // 创建BLE服务
        let service = server.create_service(uuid128!("e572775c-0df9-4b44-926b-b692e31d6971"));

        // 创建配置scene特征
        let scene_characteristic = service.lock().create_characteristic(
            uuid128!("c7d7ee2f-c84b-4f5c-a2a4-e642c97a880d"),
            NimbleProperties::READ | NimbleProperties::WRITE | NimbleProperties::NOTIFY,
        );
        let mut light = light_sender.clone();
        scene_characteristic
            .lock()
            .on_write(move |args| {
                let data = args.recv_data();
                log::warn!("data:{data:?}");
                match Scene::from_u8(data) {
                    Ok(scene) => {
                        if light.set_scene(scene).is_err() {
                            args.reject();
                            log::error!("set scene error");
                        }
                    }
                    Err(e) => {
                        args.reject();
                        log::error!("parse scene error: {:#?}", e);
                    }
                }
            })
            .on_subscribe(|characteristic, desc, _| {
                log::info!("on_subscribe: {:#?}", desc);
                characteristic.notify();
            })
            .create_2904_descriptor();

        let control_characteristic = service.lock().create_characteristic(
            uuid128!("bc00dad8-280c-49f9-9efd-3a8137594ef2"),
            NimbleProperties::WRITE,
        );

        let mut light = light_sender.clone();
        control_characteristic.lock().on_write(move |args| {
            let data = args.recv_data();
            let control = LightControl::from(data);
            let res = match control {
                LightControl::Close => light.close(),
                LightControl::Open => light.open(),
                LightControl::Reset => light.reset(),
            };
            if res.is_err() {
                args.reject();
                log::error!("control error");
            }
        });
        let state_characteristic = service.lock().create_characteristic(
            uuid128!("e192efae-9626-4767-8a27-b96cb9753e10"),
            NimbleProperties::NOTIFY | NimbleProperties::READ,
        );
        state_characteristic
            .lock()
            .on_subscribe(|characteristic, desc, _| {
                log::info!("on_subscribe: {:#?}", desc);
                characteristic.notify();
            })
            .create_2904_descriptor();

        // 同步时间特征
        let time_characteristic = service.lock().create_characteristic(
            uuid128!("9ae95835-6543-4bd0-8aec-6c48fe9fd989"),
            NimbleProperties::WRITE,
        );
        time_characteristic.lock().on_write(|args| {
            let data = args.recv_data();
            if data.len() == 8 {
                let t_ptr = data.as_ptr() as *const [u8; 8];
                let timestamp = u64::from_ne_bytes(unsafe { std::ptr::read(t_ptr) });
                let time = Duration::from_millis(timestamp);
                unsafe {
                    esp_idf_svc::sys::sntp_set_system_time(
                        time.as_secs() as u32,
                        time.subsec_nanos() / 1000,
                    )
                }
                log::warn!("set time {time:?}");
            } else {
                args.reject();
                log::error!("time error");
            }
        });

        let time_task_characteristic = service.lock().create_characteristic(
            uuid128!("f144af69-9642-97e1-d712-9448d1b450a1"),
            NimbleProperties::WRITE,
        );
        time_task_characteristic.lock().on_write(move |args| {
            let data = args.recv_data();
            match serde_json::from_slice::<TimerEvent>(data) {
                Ok(event) => {
                    time_sender.event_tx.try_send(event).unwrap();
                }
                Err(e) => {
                    args.reject();
                    log::error!("parse time task error: {:#?}", e);
                }
            };
        });
        // 配置广告数据并启动广告
        advertising.lock().set_data(
            BLEAdvertisementData::new()
                .name("ESP32")
                .add_service_uuid(uuid128!("e572775c-0df9-4b44-926b-b692e31d6971")),
        )?;

        advertising.lock().start()?;
        // 打印蓝牙服务相关日志
        server.ble_gatts_show_local();

        Ok(Self {
            scene_characteristic,
            control_characteristic,
            state_characteristic,
            time_task_characteristic,
        })
    }

    pub fn set_state(&self, state: LightState) {
        self.state_characteristic
            .lock()
            .set_value(state.into())
            .notify();
    }

    pub fn set_scene(&self, scene: &Scene) -> Result<()> {
        self.scene_characteristic
            .lock()
            .set_value(&scene.to_u8()?)
            .notify();
        Ok(())
    }

    pub fn get_state(&self) -> LightState {
        self.state_characteristic.lock().value_mut().value().into()
    }
}
