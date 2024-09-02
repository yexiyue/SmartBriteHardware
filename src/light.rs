use crate::ble::BleControl;
use crate::led::{blend_colors, WS2812RMT};
use crate::store::{Color, NvsStore, Scene};
use anyhow::Result;
use esp_idf_svc::timer::{EspAsyncTimer, EspTaskTimerService};
use futures::executor::ThreadPool;
use futures::future::abortable;
use futures::stream::AbortHandle;
use futures::task::SpawnExt;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{self, Receiver, Sender};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

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
        Ok(self.event_tx.send(LightEvent::Close)?)
    }
    pub fn open(&mut self) -> Result<()> {
        Ok(self.event_tx.send(LightEvent::Open)?)
    }
    pub fn set_scene(&mut self, scene: Scene) -> Result<()> {
        Ok(self.event_tx.send(LightEvent::SetScene(scene))?)
    }
    pub fn reset(&mut self) -> Result<()> {
        Ok(self.event_tx.send(LightEvent::Reset)?)
    }

    pub fn new_pari() -> (LightEventSender, Receiver<LightEvent>) {
        let (tx, rx) = mpsc::channel();
        (LightEventSender::new(tx), rx)
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

pub async fn open_led(
    mut async_timer: EspAsyncTimer,
    led: Arc<Mutex<WS2812RMT<'_>>>,
    color: Color,
) -> Result<(), anyhow::Error> {
    // 注意防止死锁，这里使用这种方式获取颜色是为了更快的释放锁
    match color {
        Color::Solid(solid) => {
            led.lock().unwrap().set_pixel(solid.color)?;
            Ok(())
        }
        Color::Gradient(gradient) => {
            if gradient.linear {
                let durations = gradient.get_color_durations();
                let mut current = 0usize;
                loop {
                    let index = current % durations.len();
                    let color_duration = &durations[index];
                    let instance = std::time::Instant::now();

                    while instance.elapsed() < color_duration.duration {
                        let color = blend_colors(
                            color_duration.start_color,
                            color_duration.end_color,
                            (instance.elapsed().as_millis() as f32)
                                / color_duration.duration.as_millis() as f32,
                        );
                        led.lock().unwrap().set_pixel(color)?;
                        async_timer.after(Duration::from_millis(60)).await?;
                    }
                    current += 1;
                }
            } else {
                let durations = gradient.colors.clone();
                let mut current = 0usize;
                loop {
                    let index = current % durations.len();
                    let color_duration = &durations[index];

                    led.lock().unwrap().set_pixel(color_duration.color)?;
                    async_timer
                        .after(Duration::from_secs_f32(color_duration.duration))
                        .await?;
                    current += 1;
                }
            }
        }
    }
}

pub fn handle_light_event(
    event_rx: Receiver<LightEvent>,
    ble_control: BleControl,
    nvs_store: NvsStore,
    led: Arc<Mutex<WS2812RMT<'static>>>,
    pool: ThreadPool,
) -> Result<()> {
    let timer_server = EspTaskTimerService::new()?;
    let open_task: Arc<Mutex<Option<AbortHandle>>> = Arc::new(Mutex::new(None));
    let scene = nvs_store.scene.clone();
    while let Ok(event) = event_rx.recv() {
        match event {
            LightEvent::Close => {
                #[cfg(debug_assertions)]
                log::warn!("close");
                if open_task.lock().unwrap().is_some() {
                    open_task.lock().unwrap().take().unwrap().abort();
                }
                led.lock().unwrap().close()?;
                ble_control.set_state(LightState::Closed);
            }
            LightEvent::Open => {
                #[cfg(debug_assertions)]
                log::warn!("open");
                if open_task.lock().unwrap().is_some() {
                    open_task.lock().unwrap().take().unwrap().abort();
                }

                let (future, abort_handle) = abortable(open_led(
                    timer_server.timer_async()?,
                    led.clone(),
                    scene.lock().color.clone(),
                ));
                pool.spawn(async move {
                    match future.await {
                        Ok(res) => match res {
                            Ok(_) => {
                                #[cfg(debug_assertions)]
                                log::info!("open led success");
                            }
                            Err(e) => {
                                #[cfg(debug_assertions)]
                                log::error!("open led error:{e}");
                            }
                        },
                        Err(_) => {
                            #[cfg(debug_assertions)]
                            log::warn!("open led abort");
                        }
                    }
                })
                .unwrap();
                *open_task.lock().unwrap() = Some(abort_handle);
                ble_control.set_state(LightState::Opened);
            }
            LightEvent::SetScene(scene) => {
                log::info!("scene:{scene:#?}");
                ble_control.set_scene_width_store(scene)?;
            }
            LightEvent::Reset => {
                ble_control.reset_scene()?;
            }
        }
    }
    Ok(())
}
