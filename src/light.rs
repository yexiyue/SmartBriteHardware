use crate::store::Scene;
use anyhow::Result;
use futures::channel::mpsc::{self, Receiver, Sender};
use serde::{Deserialize, Serialize};

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
        Ok(self.event_tx.try_send(LightEvent::Close)?)
    }
    pub fn open(&mut self) -> Result<()> {
        Ok(self.event_tx.try_send(LightEvent::Open)?)
    }
    pub fn set_scene(&mut self, scene: Scene) -> Result<()> {
        Ok(self.event_tx.try_send(LightEvent::SetScene(scene))?)
    }
    pub fn reset(&mut self) -> Result<()> {
        Ok(self.event_tx.try_send(LightEvent::Reset)?)
    }

    pub fn new_pari() -> (LightEventSender, Receiver<LightEvent>) {
        let (tx, rx) = mpsc::channel(10);
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
