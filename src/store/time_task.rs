use std::time::Duration;

use crate::light::LightEvent;
use anyhow::{anyhow, Ok, Result};
use chrono::{DateTime, Datelike, TimeDelta, Utc};
use esp_idf_svc::timer::{EspTimerService, Task};
use serde::{Deserialize, Serialize};

/// 获取延迟执行时间
pub trait GetDelta {
    fn get_delta(&self) -> anyhow::Result<TimeDelta>;
    fn timeout(&self) -> anyhow::Result<bool> {
        let delay = self.get_delta()?;
        if delay > TimeDelta::zero() && delay <= TimeDelta::seconds(60) {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TimeFrequency {
    Once(OnceTask),
    Day(DayTask),
    Week(WeekTask),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeTask {
    pub name: String,
    pub operation: LightEvent,
    #[serde(flatten)]
    pub frequency: TimeFrequency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnceTask {
    pub end_time: DateTime<Utc>,
}

impl GetDelta for OnceTask {
    fn get_delta(&self) -> Result<TimeDelta> {
        let now = Utc::now();
        Ok(self.end_time.signed_duration_since(now))
    }
}

impl OnceTask {
    async fn run<F>(&self, timer_service: EspTimerService<Task>, mut cb: F) -> Result<()>
    where
        F: FnMut() -> Result<()>,
    {
        let mut async_timer = timer_service.timer_async()?;
        loop {
            async_timer.after(Duration::from_secs(60)).await?;
            if self.timeout()? {
                cb()?;
                break;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayTask {
    pub delay: DateTime<Utc>,
}

impl GetDelta for DayTask {
    fn get_delta(&self) -> Result<TimeDelta> {
        let now = Utc::now();
        let time = now
            .with_time(self.delay.time())
            .single()
            .ok_or(anyhow!("Invalid time"))?;

        if time > now {
            Ok(time.signed_duration_since(now))
        } else {
            Ok(time.signed_duration_since(now) + TimeDelta::days(1))
        }
    }
}

impl DayTask {
    async fn run<F>(&self, timer_service: EspTimerService<Task>, mut cb: F) -> Result<()>
    where
        F: FnMut() -> Result<()>,
    {
        let mut async_timer = timer_service.timer_async()?;
        loop {
            async_timer.after(Duration::from_secs(60)).await?;
            if self.timeout()? {
                cb()?;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekTask {
    pub day_of_week: u32,
    pub delay: DateTime<Utc>,
}

impl GetDelta for WeekTask {
    fn get_delta(&self) -> Result<TimeDelta> {
        let now = Utc::now();
        let weekday = now.weekday().number_from_monday();
        let days_until_target = (self.day_of_week + 7 - weekday) % 7;
        let time = now
            .with_time(self.delay.time())
            .single()
            .ok_or(anyhow!("Invalid time"))?
            + TimeDelta::days(days_until_target as i64);

        if time > now {
            Ok(time.signed_duration_since(now))
        } else {
            Ok(time.signed_duration_since(now) + TimeDelta::days(7))
        }
    }
}

impl WeekTask {
    async fn run<F>(&self, timer_service: EspTimerService<Task>, mut cb: F) -> Result<()>
    where
        F: FnMut() -> Result<()>,
    {
        let mut async_timer = timer_service.timer_async()?;
        loop {
            async_timer.after(Duration::from_secs(60)).await?;
            if self.timeout()? {
                cb()?;
            }
        }
    }
}

impl TimeTask {
    pub async fn run<F>(&self, timer_service: EspTimerService<Task>, cb: F) -> Result<String>
    where
        F: FnMut() -> Result<()>,
    {
        match &self.frequency {
            TimeFrequency::Once(task) => task.run(timer_service, cb).await,
            TimeFrequency::Day(task) => task.run(timer_service, cb).await,
            TimeFrequency::Week(task) => task.run(timer_service, cb).await,
        }?;
        Ok(self.name.clone())
    }
}
