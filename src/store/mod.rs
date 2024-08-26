use anyhow::Result;
use esp32_nimble::utilities::mutex::Mutex;
use esp_idf_svc::nvs::{EspNvs, EspNvsPartition, NvsDefault};
use std::sync::Arc;

mod scene;
pub use scene::{Color, Scene};
pub mod time_task;

const SCENE: &str = "scene";
const SCENE_NAMESPACE: &str = "scene-config";

pub struct NvsStore {
    pub scene: Arc<Mutex<Scene>>,
    nvs: EspNvs<NvsDefault>,
}

impl NvsStore {
    pub fn new(nvs_partition: EspNvsPartition<NvsDefault>) -> Result<Self> {
        let nvs = EspNvs::new(nvs_partition, SCENE_NAMESPACE, true)?;
        let scene = if nvs.contains(SCENE)? {
            let len = nvs.blob_len(SCENE)?.unwrap_or(512);
            let mut data = vec![0u8; len];
            nvs.get_blob(SCENE, &mut data)?;
            Scene::from_u8(&data)?
        } else {
            Scene::default()
        };

        Ok(Self {
            scene: Arc::new(Mutex::new(scene)),
            nvs,
        })
    }

    pub fn write(&mut self) -> Result<()> {
        let data = self.scene.lock().to_u8()?;
        self.nvs.set_blob(SCENE, &data)?;
        Ok(())
    }

    pub fn reset(&mut self) -> Result<bool> {
        *self.scene.lock() = Scene::default();
        Ok(self.nvs.remove(SCENE)?)
    }
}
