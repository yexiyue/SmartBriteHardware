use crate::{
    light::{LightEvent, LightEventSender, LightState},
    store::{time_task::TimeTask, NvsStore, Scene},
    timer::{TimerEvent, TimerEventSender},
    transmission::Transmission,
};
use anyhow::Result;
use esp32_nimble::{
    utilities::mutex::Mutex, uuid128, BLEAdvertisementData, BLEDevice, NimbleProperties,
};
use futures::executor::ThreadPool;
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub struct BleControl {
    pub nvs_store: NvsStore,
    pub scene_transmission: Transmission,
    pub control_characteristic: Arc<Mutex<esp32_nimble::BLECharacteristic>>,
    pub state_characteristic: Arc<Mutex<esp32_nimble::BLECharacteristic>>,
    pub time_task_transmission: Transmission,
}

impl BleControl {
    pub fn new(
        nvs_store: NvsStore,
        light_sender: LightEventSender,
        mut time_sender: TimerEventSender,
        pool: ThreadPool,
    ) -> Result<Self> {
        // 获取BLE设备实例
        let device = BLEDevice::take();

        // 获取并配置BLE的广告实例
        let advertising = device.get_advertising();

        // 获取并配置BLE的服务实例。
        let server = device.get_server();

        // 配置BLE连接时的回调函数
        server.on_connect(|server, desc| {
            #[cfg(debug_assertions)]
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
            #[cfg(debug_assertions)]
            log::warn!("on_disconnect: {:#?}, reason: {:#?}", desc, reason)
        });

        // 创建BLE服务
        let service = server.create_service(uuid128!("e572775c-0df9-4b44-926b-b692e31d6971"));

        // 场景服务
        let scene_transmission = Transmission::new(
            service.clone(),
            uuid128!("c7d7ee2f-c84b-4f5c-a2a4-e642c97a880d"),
            pool.clone(),
        );
        let nvs_store_clone = nvs_store.clone();
        scene_transmission.init(Some(move |data: Vec<u8>, transmission: &Transmission| {
            let data = serde_json::from_slice::<Scene>(&data)?;
            *nvs_store_clone.scene.lock() = data;
            nvs_store_clone.write_scene()?;
            transmission.notify_update();
            Ok(())
        }));

        let control_characteristic = service.lock().create_characteristic(
            uuid128!("bc00dad8-280c-49f9-9efd-3a8137594ef2"),
            NimbleProperties::WRITE,
        );

        let light = light_sender.clone();
        control_characteristic.lock().on_write(move |args| {
            let data = args.recv_data();
            let control = LightEvent::from(data);

            if light.event_tx.send(control).is_err() {
                args.reject();
                #[cfg(debug_assertions)]
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
                #[cfg(debug_assertions)]
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
                let now = chrono::Utc::now().to_rfc3339();
                #[cfg(debug_assertions)]
                log::warn!("set time {now}");
            } else {
                args.reject();
                #[cfg(debug_assertions)]
                log::error!("time error");
            }
        });

        // 定时任务服务
        let time_task_transmission = Transmission::new(
            service.clone(),
            uuid128!("f144af69-9642-97e1-d712-9448d1b450a1"),
            pool,
        );
        time_task_transmission.init(Some(move |data: Vec<u8>, _: &Transmission| {
            let event = serde_json::from_slice::<TimerEvent>(&data)?;
            log::warn!("time task event: {:?}", event);
            time_sender.event_tx.try_send(event)?;
            Ok(())
        }));

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
            nvs_store,
            scene_transmission,
            control_characteristic,
            state_characteristic,
            time_task_transmission,
        })
    }

    pub fn set_state(&self, state: LightState) {
        self.state_characteristic
            .lock()
            .set_value(state.into())
            .notify();
    }

    pub fn set_scene(&self, scene: &Scene) -> Result<()> {
        self.scene_transmission.set_value(scene.to_u8()?)?;
        Ok(())
    }

    pub fn set_timer(&self, time_task: &[TimeTask]) -> Result<()> {
        self.time_task_transmission
            .set_value(serde_json::to_vec(time_task)?)?;
        Ok(())
    }

    pub fn get_state(&self) -> LightState {
        self.state_characteristic.lock().value_mut().value().into()
    }

    pub fn init(&self) -> Result<()> {
        self.set_timer(&self.nvs_store.time_task.lock())?;
        self.set_scene(&self.nvs_store.scene.lock())?;
        self.set_state(LightState::Closed);
        Ok(())
    }

    pub fn set_timer_with_store(&self) -> Result<()> {
        self.set_timer(&self.nvs_store.time_task.lock())?;
        self.nvs_store.write_time_task()?;
        Ok(())
    }

    pub fn reset_scene(&self) -> Result<()> {
        self.nvs_store.reset_scene()?;
        self.set_scene(&self.nvs_store.scene.lock())?;
        Ok(())
    }
}
