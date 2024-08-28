use crate::{
    ble::BleControl,
    light::{LightEventSender, LightState},
};
use anyhow::Result;
use esp_idf_svc::hal::{
    gpio::{Input, InputPin, InterruptType, OutputPin, PinDriver, Pull},
    task::notification::Notification,
};
use std::num::NonZeroU32;

pub struct Button<T>
where
    T: InputPin + OutputPin,
{
    button: PinDriver<'static, T, Input>,
    ble_control: BleControl,
    light_event_sender: LightEventSender,
}

impl<T> Button<T>
where
    T: InputPin + OutputPin,
{
    pub fn new(
        pin: T,
        ble_control: BleControl,
        light_event_sender: LightEventSender,
    ) -> Result<Self> {
        Ok(Self {
            button: PinDriver::input(pin)?,
            ble_control,
            light_event_sender,
        })
    }

    pub fn init(mut self) -> Result<()> {
        self.button.set_pull(Pull::Up)?;
        self.button.set_interrupt_type(InterruptType::PosEdge)?;

        std::thread::spawn(move || -> Result<(), anyhow::Error> {
            let notification = Notification::new();
            let notifier = notification.notifier();
            unsafe {
                self.button.subscribe(move || {
                    notifier.notify_and_yield(NonZeroU32::new(1).unwrap());
                })?;
            }

            loop {
                self.button.enable_interrupt()?;
                notification.wait(esp_idf_svc::hal::delay::BLOCK);
                let state = self.ble_control.get_state();
                match state {
                    LightState::Closed => {
                        self.light_event_sender.open()?;
                    }
                    LightState::Opened => {
                        self.light_event_sender.close()?;
                    }
                }
            }
        });
        Ok(())
    }
}
