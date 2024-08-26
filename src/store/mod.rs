use anyhow::Result;
use esp32_nimble::utilities::mutex::Mutex;
use esp_idf_svc::nvs::{EspNvs, EspNvsPartition, NvsDefault};
use std::sync::Arc;

mod scene;
pub use scene::{Color, Scene};
pub mod time_task;

const SCENE: &str = "scene";
const TIME_TASK: &str = "time_task";
const NAMESPACE: &str = "config";

#[derive(Clone)]
pub struct NvsStore {
    pub scene: Arc<Mutex<Scene>>,
    pub time_task: Arc<Mutex<Vec<time_task::TimeTask>>>,
    pub nvs: Arc<Mutex<EspNvs<NvsDefault>>>,
}

impl NvsStore {
    pub fn new(nvs_partition: EspNvsPartition<NvsDefault>) -> Result<Self> {
        let nvs = EspNvs::new(nvs_partition, NAMESPACE, true)?;
        let scene = if nvs.contains(SCENE)? {
            let len = nvs.blob_len(SCENE)?.unwrap_or(512);
            let mut data = vec![0u8; len];
            nvs.get_blob(SCENE, &mut data)?;
            Scene::from_u8(&data)?
        } else {
            Scene::default()
        };
        let time_task = if nvs.contains(TIME_TASK)? {
            let len = nvs.blob_len(TIME_TASK)?.unwrap_or(512);
            let mut data = vec![0u8; len];
            nvs.get_blob(TIME_TASK, &mut data)?;
            serde_json::from_slice(&data)?
        } else {
            vec![]
        };

        Ok(Self {
            scene: Arc::new(Mutex::new(scene)),
            time_task: Arc::new(Mutex::new(time_task)),
            nvs: Arc::new(Mutex::new(nvs)),
        })
    }

    pub fn write_scene(&self) -> Result<()> {
        let data = self.scene.lock().to_u8()?;
        self.nvs.lock().set_blob(SCENE, &data)?;
        Ok(())
    }

    pub fn reset_scene(&self) -> Result<bool> {
        *self.scene.lock() = Scene::default();
        Ok(self.nvs.lock().remove(SCENE)?)
    }

    pub fn write_time_task(&self) -> Result<()> {
        let data = serde_json::to_vec(&*self.time_task.lock())?;
        self.nvs.lock().set_blob(TIME_TASK, &data)?;
        Ok(())
    }
}
