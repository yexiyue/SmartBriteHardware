use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::sleep,
    time::Duration,
};

use smart_brite::{
    ble::{BleControl, LightEvent, LightEventSender},
    led::{blend_colors, WS2812RMT},
    store::{Color, NvsScene},
};

fn main() -> anyhow::Result<()> {
    let (_sys_loop, peripherals, nvs_partition) = smart_brite::init()?;
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let led = Arc::new(Mutex::new(WS2812RMT::new(
        peripherals.pins.gpio8,
        peripherals.rmt.channel0,
    )?));
    let mut nvs_scene = NvsScene::new(nvs_partition)?;
    let light_event_sender = LightEventSender::new(event_tx);
    let ble_control = BleControl::new(light_event_sender)?;

    ble_control
        .scene_characteristic
        .lock()
        .set_value(&nvs_scene.scene.to_u8()?);

    let ble_control_clone = ble_control.clone();

    std::thread::spawn(move || {
        ble_control_clone
            .state_characteristic
            .lock()
            .set_value(b"closed");
        loop {
            ble_control_clone.state_characteristic.lock().notify();
            sleep(Duration::from_secs(1));
        }
    });
    let state = Arc::new(AtomicBool::new(false));

    while let Ok(event) = event_rx.recv() {
        match event {
            LightEvent::Close => {
                state.store(false, Ordering::Relaxed);
                led.lock().unwrap().close()?;
                ble_control
                    .state_characteristic
                    .lock()
                    .set_value(b"closed")
                    .notify();
            }
            LightEvent::Open => {
                state.store(true, Ordering::Relaxed);
                match &nvs_scene.scene.color {
                    Color::Solid(solid) => {
                        led.lock().unwrap().set_pixel(solid.color)?;
                        ble_control
                            .state_characteristic
                            .lock()
                            .set_value(b"opened")
                            .notify();
                    }
                    Color::Gradient(gradient) => {
                        let led = led.clone();
                        let state = state.clone();
                        if gradient.linear {
                            let durations = gradient.get_color_durations();
                            std::thread::spawn(move || {
                                let mut current = 0usize;
                                loop {
                                    let index = current % durations.len();
                                    let color_duration = &durations[index];
                                    let instance = std::time::Instant::now();
                                    let is_open = state.load(Ordering::Relaxed);
                                    if !is_open {
                                        led.lock().unwrap().close()?;
                                        break;
                                    }
                                    while instance.elapsed() < color_duration.duration
                                        && state.load(Ordering::Relaxed)
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
                                Ok::<(), anyhow::Error>(())
                            });
                        } else {
                            let durations = gradient.colors.clone();
                            std::thread::spawn(move || {
                                let mut current = 0usize;
                                loop {
                                    let index = current % durations.len();
                                    let color_duration = &durations[index];
                                    let is_open = state.load(Ordering::Relaxed);
                                    if !is_open {
                                        led.lock().unwrap().close()?;
                                        break;
                                    }
                                    led.lock().unwrap().set_pixel(color_duration.color)?;
                                    sleep(Duration::from_secs_f32(color_duration.duration));
                                    current += 1;
                                }
                                Ok::<(), anyhow::Error>(())
                            });
                        }

                        ble_control
                            .state_characteristic
                            .lock()
                            .set_value(b"opened")
                            .notify();
                    }
                }
            }
            LightEvent::SetScene(scene) => {
                log::info!("scene:{scene:#?}");
                nvs_scene.scene = scene;
                nvs_scene.write()?;
                ble_control
                    .scene_characteristic
                    .lock()
                    .set_value(&nvs_scene.scene.to_u8()?);
            }
            LightEvent::Reset => {
                nvs_scene.reset()?;
                ble_control
                    .scene_characteristic
                    .lock()
                    .set_value(&nvs_scene.scene.to_u8()?);
            }
        }
    }
    Ok(())
}
