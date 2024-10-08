use anyhow::Result;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;

pub mod ble;
pub mod button;
pub mod led;
pub mod light;
pub mod store;
pub mod timer;
pub mod transmission;

pub fn init() -> Result<(EspSystemEventLoop, Peripherals, EspDefaultNvsPartition)> {
    // 链接SDK中的补丁，以修正某些功能的兼容性问题。
    esp_idf_svc::sys::link_patches();

    // 初始化日志系统，为后续的调试和错误追踪提供支持。
    esp_idf_svc::log::EspLogger::initialize_default();

    // 获取系统事件循环实例，用于处理系统级别的事件。
    let sys_loop = esp_idf_svc::eventloop::EspSystemEventLoop::take()?;

    // 获取外设句柄，用于访问和控制硬件资源。
    let peripherals = Peripherals::take()?;

    // 获取默认的NVS分区，用于存储配置数据和运行时信息。
    let nvs_partition = EspDefaultNvsPartition::take()?;

    // 返回初始化完成的系统事件循环、外设句柄和默认NVS分区。
    Ok((sys_loop, peripherals, nvs_partition))
}
