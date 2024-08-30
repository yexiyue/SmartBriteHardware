use futures::{executor::ThreadPool, task::SpawnExt};
use smart_brite::{
    ble::BleControl,
    button::Button,
    led::WS2812RMT,
    light::{handle_light_event, LightEventSender},
    store::NvsStore,
    timer::{TimeTaskManager, TimerEventSender},
};
use std::sync::{Arc, Mutex};

fn main() -> anyhow::Result<()> {
    let (_sys_loop, peripherals, nvs_partition) = smart_brite::init()?;

    let led = Arc::new(Mutex::new(WS2812RMT::new(
        peripherals.pins.gpio8,
        peripherals.rmt.channel0,
    )?));

    let pool = ThreadPool::new()?;

    let nvs_store = NvsStore::new(nvs_partition)?;

    let (light_event_sender, event_rx) = LightEventSender::new_pari();
    let (timer_event_sender, time_event_rx) = TimerEventSender::new_pair();

    let time_task_manager = TimeTaskManager::new(
        nvs_store.time_task.clone(),
        light_event_sender.clone(),
        pool.clone(),
    );

    let ble_control = BleControl::new(
        nvs_store.clone(),
        light_event_sender.clone(),
        timer_event_sender,
    )?;
    let button = Button::new(
        peripherals.pins.gpio9,
        ble_control.clone(),
        light_event_sender,
    )?;
    time_task_manager.event(time_event_rx, ble_control.clone())?;
    ble_control.init()?;
    button.init()?;

    pool.clone().spawn(async move {
        match handle_light_event(event_rx, ble_control, nvs_store, led, pool).await {
            Ok(_) => {}
            Err(e) => {
                log::error!("handle_light_event error: {}", e);
            }
        }
    })?;

    time_task_manager.run()?;
    Ok(())
}
