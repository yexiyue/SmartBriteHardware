use std::time::Duration;

use anyhow::Result;
use esp_idf_svc::nvs::{EspNvs, EspNvsPartition, NvsDefault};
use rgb::RGB8;
use serde::{Deserialize, Serialize};

const SCENE: &str = "scene";
const SCENE_NAMESPACE: &str = "scene-config";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Solid {
    pub color: RGB8,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GradientColorItem {
    pub color: RGB8,
    pub duration: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Gradient {
    pub colors: Vec<GradientColorItem>,
    #[serde(default)]
    pub linear: bool,
}

#[derive(Debug, Clone)]
pub struct ColorDuration {
    pub start_color: RGB8,
    pub end_color: RGB8,
    pub duration: Duration,
}

impl Gradient {
    pub fn get_color_durations(&self) -> Vec<ColorDuration> {
        let mut last_color = self.colors.last().unwrap();
        let color_durations = self
            .colors
            .iter()
            .map(|g| {
                let color_duration = ColorDuration {
                    start_color: last_color.color,
                    end_color: g.color,
                    duration: Duration::from_secs_f32(g.duration),
                };
                last_color = g;
                color_duration
            })
            .collect();
        color_durations
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum Color {
    Solid(Solid),
    Gradient(Gradient),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub name: String,
    pub auto_on: bool,
    #[serde(flatten)]
    pub color: Color,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            auto_on: false,
            color: Color::Solid(Solid {
                color: RGB8::new(255, 255, 255),
            }),
        }
    }
}

impl Scene {
    pub fn from_u8(data: &[u8]) -> Result<Self> {
        Ok(serde_json::from_slice(data)?)
    }

    pub fn to_u8(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }
}

pub struct NvsScene {
    pub scene: Scene,
    nvs: EspNvs<NvsDefault>,
}

impl NvsScene {
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

        Ok(Self { scene, nvs })
    }

    pub fn write(&mut self) -> Result<()> {
        let data = self.scene.to_u8()?;
        self.nvs.set_blob(SCENE, &data)?;
        Ok(())
    }

    pub fn reset(&mut self) -> Result<bool> {
        self.scene = Scene::default();
        Ok(self.nvs.remove(SCENE)?)
    }
}
