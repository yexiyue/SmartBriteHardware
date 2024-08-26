use std::{
    num::NonZeroU32,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread::sleep,
    time::Duration,
};

use chrono::{TimeDelta, Utc};
use esp32_nimble::utilities::mutex::Mutex as NimbleMutex;
use esp_idf_svc::hal::{
    gpio::{InterruptType, PinDriver, Pull},
    task::notification::Notification,
};
use futures::{channel::mpsc, executor::LocalPool, task::SpawnExt, StreamExt};
use smart_brite::{
    ble::{BleControl, LightControl, LightEvent, LightEventSender, LightState},
    led::{blend_colors, WS2812RMT},
    store::{
        time_task::{OnceTask, TimeFrequency, TimeTask},
        Color, NvsStore,
    },
    timer::{TimeTaskManager, TimerEventSender},
};

fn main() -> anyhow::Result<()> {
    let (_sys_loop, peripherals, nvs_partition) = smart_brite::init()?;
    let (event_tx, mut event_rx) = mpsc::channel(10);
    let (time_event_tx, time_event_rx) = mpsc::channel(10);
    let (tx, rx) = std::sync::mpsc::channel::<LightState>();
    let led = Arc::new(Mutex::new(WS2812RMT::new(
        peripherals.pins.gpio8,
        peripherals.rmt.channel0,
    )?));
    let mut pool = LocalPool::new();
    let mut nvs_scene = NvsStore::new(nvs_partition)?;
    let light_event_sender = LightEventSender::new(event_tx);
    let timer_event_sender = TimerEventSender::new(time_event_tx);
    let time_task_manager = TimeTaskManager::new(
        Arc::new(NimbleMutex::new(vec![
            TimeTask {
                name: "default".into(),
                operation: LightControl::Open,
                frequency: TimeFrequency::Once(OnceTask {
                    end_time: Utc::now()
                        .checked_add_signed(TimeDelta::seconds(5))
                        .unwrap(),
                }),
            },
            TimeTask {
                name: "default2".into(),
                operation: LightControl::Close,
                frequency: TimeFrequency::Once(OnceTask {
                    end_time: Utc::now()
                        .checked_add_signed(TimeDelta::seconds(7))
                        .unwrap(),
                }),
            },
        ])),
        light_event_sender.clone(),
        pool.spawner(),
    );
    time_task_manager.event(time_event_rx)?;

    let mut light_event_sender_clone = light_event_sender.clone();
    let ble_control = BleControl::new(light_event_sender, timer_event_sender)?;

    ble_control.set_scene(&nvs_scene.scene.lock())?;
    ble_control.set_state(LightState::Closed);
    let ble_control_clone = ble_control.clone();
    let ble_control_clone2 = ble_control.clone();
    let scene = nvs_scene.scene.clone();

    // 标识位，用于退出loop循环
    let flag = Arc::new(AtomicUsize::new(0));
    let flag_clone = flag.clone();

    let mut button = PinDriver::input(peripherals.pins.gpio9)?;
    button.set_pull(Pull::Up)?;
    button.set_interrupt_type(InterruptType::PosEdge)?;

    std::thread::spawn(move || -> Result<(), anyhow::Error> {
        let notification = Notification::new();
        let notifier = notification.notifier();
        unsafe {
            button.subscribe(move || {
                notifier.notify_and_yield(NonZeroU32::new(1).unwrap());
            })?;
        }

        loop {
            button.enable_interrupt()?;
            notification.wait(esp_idf_svc::hal::delay::BLOCK);
            let state = ble_control_clone2.get_state();
            match state {
                LightState::Closed => {
                    light_event_sender_clone.open()?;
                }
                LightState::Opened => {
                    light_event_sender_clone.close()?;
                }
            }
        }
    });

    // 专门开一个线程处理灯的状态，通过channel信道通信
    std::thread::spawn(move || {
        while let Ok(light_state) = rx.recv() {
            match light_state {
                LightState::Closed => {
                    led.lock().unwrap().close()?;
                }
                LightState::Opened => {
                    // 注意防止死锁，这里使用这种方式获取颜色是为了更快的释放锁
                    let color = scene.lock().color.clone();
                    match color {
                        Color::Solid(solid) => {
                            led.lock().unwrap().set_pixel(solid.color)?;
                        }
                        Color::Gradient(gradient) => {
                            let led = led.clone();
                            let gradient_state = flag.clone();
                            let current_id = gradient_state.load(Ordering::Relaxed);
                            if gradient.linear {
                                let durations = gradient.get_color_durations();
                                let mut current = 0usize;
                                loop {
                                    let index = current % durations.len();
                                    let color_duration = &durations[index];
                                    let instance = std::time::Instant::now();
                                    if current_id != gradient_state.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    while instance.elapsed() < color_duration.duration
                                        && current_id == gradient_state.load(Ordering::Relaxed)
                                    {
                                        let color = blend_colors(
                                            color_duration.start_color,
                                            color_duration.end_color,
                                            (instance.elapsed().as_millis() as f32)
                                                / color_duration.duration.as_millis() as f32,
                                        );
                                        led.lock().unwrap().set_pixel(color)?;
                                        sleep(Duration::from_millis(60));
                                    }
                                    current += 1;
                                }
                            } else {
                                let durations = gradient.colors.clone();
                                let mut current = 0usize;
                                loop {
                                    let index = current % durations.len();
                                    let color_duration = &durations[index];
                                    let is_open = gradient_state.load(Ordering::Relaxed);
                                    if is_open != current_id {
                                        break;
                                    }
                                    led.lock().unwrap().set_pixel(color_duration.color)?;
                                    sleep(Duration::from_secs_f32(color_duration.duration));
                                    current += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok::<(), anyhow::Error>(())
    });

    pool.spawner().spawn(async move {
        while let Some(event) = event_rx.next().await {
            match event {
                LightEvent::Close => {
                    log::warn!("close");
                    flag_clone.fetch_add(1, Ordering::Relaxed);
                    ble_control_clone.set_state(LightState::Closed);
                    tx.send(LightState::Closed).unwrap();
                }
                LightEvent::Open => {
                    log::warn!("open");
                    flag_clone.fetch_add(1, Ordering::Relaxed);
                    ble_control_clone.set_state(LightState::Opened);
                    tx.send(LightState::Opened).unwrap();
                }
                LightEvent::SetScene(scene) => {
                    log::info!("scene:{scene:#?}");
                    *nvs_scene.scene.lock() = scene;
                    nvs_scene.write().unwrap();
                    ble_control_clone
                        .set_scene(&nvs_scene.scene.lock())
                        .unwrap();
                }
                LightEvent::Reset => {
                    nvs_scene.reset().unwrap();
                    ble_control_clone
                        .set_scene(&nvs_scene.scene.lock())
                        .unwrap();
                }
            }
        }
    })?;
    time_task_manager.run()?;
    pool.run();
    Ok(())
}
