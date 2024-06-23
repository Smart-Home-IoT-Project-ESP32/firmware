use core::time::Duration;

use esp_idf_sys::ESP_NOW_MAX_DATA_LEN;

pub const MAX_DATA_LEN: usize = ESP_NOW_MAX_DATA_LEN as usize;
// Need lots of stack to parse JSON
pub const STACK_SIZE: usize = 10240;
/// AP SSID
pub const SSID: &str = "Smart Home Hub";
/// Default TCP server address (telegraf)
pub const TCP_SERVER_ADDR: &str = "tcp://4.232.184.193:8094";
/// Broadcast ping frequency (interval)
pub const BROADCAST_PING_INTERVAL: Duration = Duration::from_secs(2);
/// Sd retry frequency (interval)
pub const SD_RETRY_INTERVAL: Duration = Duration::from_secs(2);
/// WiFi retry frequency (interval)
pub const WIFI_RETRY_INTERVAL: Duration = Duration::from_secs(5);
/// ESP-NOW initialization time limit
pub const ESP_NOW_INIT_TIMEOUT: Duration = Duration::from_secs(10);
