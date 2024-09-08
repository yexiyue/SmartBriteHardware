## SmartBrite

## 项目简介

SmartBrite 是一个基于蓝牙技术的智能灯光控制系统。本项目旨在通过蓝牙实现与移动设备的交互，以控制灯光的颜色以及执行定时任务。该项目是在学习完 [Embedded Rust on Espressif](https://narukara.github.io/std-training-zh-cn/01_intro.html) 教程之后进行的实践应用。

## 功能特性

- **蓝牙通信**：支持通过蓝牙协议与客户端进行数据交换，以便远程控制。
- **颜色控制**：能够调节灯光的颜色，包括固定颜色显示及颜色渐变效果。
- **定时任务**：具备设置定时开关灯的功能，用户可以指定某一时刻自动开启或关闭灯光。

## 技术栈

- **编程语言**: Rust
- **硬件平台**: ESP32C3
- **库**
  - `esp-idf-svc` 提供了与 ESP-IDF 系统集成的服务。
  - `esp32-nimble` 提供 BLE 功能，使 ESP32 能够作为 BLE 设备运行，支持 BLE 的基本操作，如广告、连接等。
  - `serde` 及 `serde_json` 用于数据序列化。
  - `rgb` 库用于处理 RGB 颜色。
  - `chrono` 用于处理日期和时间。
  - `futures` 提供异步编程支持。
  - `rand` 用于生成随机数。

## 贡献指南

欢迎贡献者！如果您有任何改进建议或发现 bug，请随时提交 issue 或 pull request。

## 许可证

本项目采用 MIT 许可证。详情见 LICENSE 文件。

