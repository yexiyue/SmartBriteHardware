use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread::sleep,
    time::Duration,
};

use smart_brite::{
    ble::{BleControl, LightEvent, LightEventSender, LightState},
    led::{blend_colors, WS2812RMT},
    store::{Color, NvsScene},
};

fn main() -> anyhow::Result<()> {
    let (_sys_loop, peripherals, nvs_partition) = smart_brite::init()?;
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let (tx, rx) = std::sync::mpsc::channel::<LightState>();
    let led = Arc::new(Mutex::new(WS2812RMT::new(
        peripherals.pins.gpio8,
        peripherals.rmt.channel0,
    )?));
    let mut nvs_scene = NvsScene::new(nvs_partition)?;
    let light_event_sender = LightEventSender::new(event_tx);
    let ble_control = BleControl::new(light_event_sender)?;

    ble_control.set_scene(&nvs_scene.scene.lock())?;
    ble_control.set_state(LightState::Closed);
    let ble_control_clone = ble_control.clone();
    let scene = nvs_scene.scene.clone();

    // 标识位，用于退出loop循环
    let flag = Arc::new(AtomicUsize::new(0));
    let flag_clone = flag.clone();

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

    while let Ok(event) = event_rx.recv() {
        match event {
            LightEvent::Close => {
                log::warn!("close");
                flag_clone.fetch_add(1, Ordering::Relaxed);
                ble_control_clone.set_state(LightState::Closed);
                tx.send(LightState::Closed)?;
            }
            LightEvent::Open => {
                log::warn!("open");
                flag_clone.fetch_add(1, Ordering::Relaxed);
                ble_control_clone.set_state(LightState::Opened);
                tx.send(LightState::Opened)?;
            }
            LightEvent::SetScene(scene) => {
                log::info!("scene:{scene:#?}");
                *nvs_scene.scene.lock() = scene;
                nvs_scene.write()?;
                ble_control_clone.set_scene(&nvs_scene.scene.lock())?;
            }
            LightEvent::Reset => {
                nvs_scene.reset()?;
                ble_control_clone.set_scene(&nvs_scene.scene.lock())?;
            }
        }
    }

    Ok(())
}
