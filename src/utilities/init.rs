use esp_idf_sys::{esp_task_wdt_init, link_patches};
use log::info;

pub fn init() {
    // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // or else some patches to the runtime implemented by esp-idf-sys might not link properly.
    link_patches(); //expand maxsize to 20000 and enable buildscript.
    esp_idf_svc::log::EspLogger::initialize_default();
    // Set up the watchdog timer, not panicking if it triggers.
    watchdog_reconfigure(10000, false);
    info!("Logger initialised.");
}

pub fn watchdog_timeout(timeout: u32, has_to_panic: bool) {
    let config = &esp_idf_sys::esp_task_wdt_config_t {
        timeout_ms: timeout,
        trigger_panic: has_to_panic,
        ..Default::default()
    } as *const _;
    unsafe {
        esp_task_wdt_init(config);
    }
}
pub fn watchdog_reconfigure(timeout: u32, has_to_panic: bool) {
    let wtd_config = &esp_idf_sys::esp_task_wdt_config_t {
        timeout_ms: timeout,
        trigger_panic: has_to_panic,
        ..Default::default()
    } as *const _;
    unsafe {
        esp_idf_sys::esp_task_wdt_reconfigure(wtd_config);
    }
}
