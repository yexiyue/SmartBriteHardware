use crate::{
    ble::{BleControl, LightControl, LightEventSender},
    store::time_task::TimeTask,
};
use anyhow::Result;
use esp32_nimble::utilities::mutex::Mutex;
use esp_idf_svc::timer::{EspTaskTimerService, EspTimerService, Task};
use futures::{channel::mpsc, executor::LocalSpawner, task::SpawnExt, StreamExt};
use futures::{future::abortable, stream::AbortHandle};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", tag = "type", content = "data")]
pub enum TimerEvent {
    AddTask(TimeTask),
    RemoveTask(String),
}

#[derive(Debug, Clone)]
pub struct TimerEventSender {
    pub event_tx: mpsc::Sender<TimerEvent>,
}

impl TimerEventSender {
    pub fn new(event_tx: mpsc::Sender<TimerEvent>) -> Self {
        Self { event_tx }
    }

    pub fn add_task(&mut self, time_task: TimeTask) -> Result<()> {
        Ok(self.event_tx.try_send(TimerEvent::AddTask(time_task))?)
    }

    pub fn remove_task(&mut self, name: String) -> Result<()> {
        Ok(self.event_tx.try_send(TimerEvent::RemoveTask(name))?)
    }

    pub fn new_pair() -> (TimerEventSender, mpsc::Receiver<TimerEvent>) {
        let (tx, rx) = mpsc::channel(10);
        (TimerEventSender::new(tx), rx)
    }
}

#[derive(Clone)]
pub struct TimeTaskManager {
    pub tasks: Arc<Mutex<Vec<TimeTask>>>,
    pub light_event_sender: LightEventSender,
    pub timer_service: EspTimerService<Task>,
    pub abort_handles: Arc<Mutex<HashMap<String, AbortHandle>>>,
    pub spawner: LocalSpawner,
}

unsafe impl Send for TimeTaskManager {}

impl TimeTaskManager {
    pub fn new(
        tasks: Arc<Mutex<Vec<TimeTask>>>,
        light_event_sender: LightEventSender,
        spawner: LocalSpawner,
    ) -> Self {
        Self {
            light_event_sender,
            tasks,
            abort_handles: Arc::new(Mutex::new(HashMap::new())),
            timer_service: EspTaskTimerService::new().unwrap(),
            spawner,
        }
    }

    pub fn run(&self) -> Result<()> {
        let tasks = self.tasks.lock().clone();
        for time_task in tasks {
            self.add_task(time_task)?;
        }
        Ok(())
    }

    pub fn abort(&self, name: &str) {
        if let Some(abort_handle) = self.abort_handles.lock().remove(name) {
            abort_handle.abort();
        }
        let index = self.tasks.lock().iter().position(|item| item.name == name);
        if index.is_some() {
            self.tasks.lock().remove(index.unwrap());
        }
    }

    fn add_task(&self, time_task: TimeTask) -> Result<()> {
        let time_task_name = time_task.name.clone();
        let index = self
            .tasks
            .lock()
            .iter()
            .position(|item| item.name == time_task_name);
        // 查看任务中是否存在，存在就中断并删除
        if index.is_some() {
            self.abort(&time_task_name);
        }
        self.tasks.lock().push(time_task.clone());

        let mut light_event_sender = self.light_event_sender.clone();
        let timer_service = self.timer_service.clone();
        let control = time_task.operation.clone();

        let (future, abort_handle) = abortable(async move {
            time_task
                .run(timer_service, || match control {
                    LightControl::Close => light_event_sender.close(),
                    LightControl::Open => light_event_sender.open(),
                    LightControl::Reset => unreachable!(),
                })
                .await
        });

        self.abort_handles
            .lock()
            .insert(time_task_name, abort_handle);
        self.spawner.spawn(async {
            match future.await {
                Ok(res) => {
                    log::info!("Timer task {:?} finished", res);
                }
                Err(e) => {
                    log::warn!("Timer task  aborted: {}", e);
                }
            }
        })?;

        Ok(())
    }

    pub fn event(
        &self,
        mut task_rx: mpsc::Receiver<TimerEvent>,
        ble_control: BleControl,
    ) -> Result<()> {
        let manager = self.clone();
        self.spawner.spawn(async move {
            if let Some(event) = task_rx.next().await {
                match event {
                    TimerEvent::AddTask(time_task) => {
                        manager.add_task(time_task).unwrap();
                    }
                    TimerEvent::RemoveTask(name) => {
                        manager.abort(&name);
                    }
                }
                ble_control.set_timer_with_store().unwrap();
            }
        })?;
        Ok(())
    }
}
