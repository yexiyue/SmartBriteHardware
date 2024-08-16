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
    let led = Arc::new(Mutex::new(WS2812RMT::new(
        peripherals.pins.gpio8,
        peripherals.rmt.channel0,
    )?));
    let mut nvs_scene = NvsScene::new(nvs_partition)?;
    let light_event_sender = LightEventSender::new(event_tx);
    let ble_control = BleControl::new(light_event_sender)?;

    ble_control.set_scene(&nvs_scene.scene)?;
    ble_control.set_state(LightState::Opened);

    let gradient_state = Arc::new(AtomicUsize::new(0));

    while let Ok(event) = event_rx.recv() {
        match event {
            LightEvent::Close => {
                gradient_state.fetch_add(1, Ordering::Relaxed);
                led.lock().unwrap().close()?;
                ble_control.set_state(LightState::Closed);
            }
            LightEvent::Open => match &nvs_scene.scene.color {
                Color::Solid(solid) => {
                    gradient_state.fetch_add(1, Ordering::Relaxed);
                    led.lock().unwrap().set_pixel(solid.color)?;
                    ble_control.set_state(LightState::Opened);
                }
                Color::Gradient(gradient) => {
                    let led = led.clone();
                    let gradient_state = gradient_state.clone();
                    let current_id = gradient_state.load(Ordering::Relaxed) + 1;
                    if gradient.linear {
                        let durations = gradient.get_color_durations();
                        std::thread::spawn(move || {
                            let mut current = 0usize;
                            gradient_state.fetch_add(1, Ordering::Relaxed);
                            loop {
                                let index = current % durations.len();
                                let color_duration = &durations[index];
                                let instance = std::time::Instant::now();
                                if current_id != gradient_state.load(Ordering::Relaxed) {
                                    led.lock().unwrap().close()?;
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
                            Ok::<(), anyhow::Error>(())
                        });
                    } else {
                        let durations = gradient.colors.clone();
                        std::thread::spawn(move || {
                            let mut current = 0usize;
                            gradient_state.fetch_add(1, Ordering::Relaxed);
                            loop {
                                let index = current % durations.len();
                                let color_duration = &durations[index];
                                let is_open = gradient_state.load(Ordering::Relaxed);
                                if is_open != current_id {
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

                    ble_control.set_state(LightState::Opened);
                }
            },
            LightEvent::SetScene(scene) => {
                log::info!("scene:{scene:#?}");
                nvs_scene.scene = scene;
                nvs_scene.write()?;
                ble_control.set_scene(&nvs_scene.scene)?;
            }
            LightEvent::Reset => {
                nvs_scene.reset()?;
                ble_control.set_scene(&nvs_scene.scene)?;
            }
        }
    }
    Ok(())
}
