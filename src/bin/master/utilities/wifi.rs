use core::time::Duration;
use std::{thread, time::SystemTime};

use esp_idf_svc::sntp;
use log::{info, warn};

use crate::utilities;

use super::{constants::WIFI_RETRY_INTERVAL, global_state::GlobalState};

/// Task that handles WiFi connection and reconnects if disconnected.
pub fn connection_task() {
    let gs = GlobalState::get();
    let mut last_try_reconnect: Option<SystemTime> = None;
    loop {
        thread::sleep(Duration::from_millis(500));

        // Skip if Wi-Fi is not initialized yet
        if gs.wifi.lock().unwrap().is_none() {
            continue;
        }
        if !is_connected() {
            // Try to connect
            if last_try_reconnect.is_none()
                || last_try_reconnect
                    .unwrap()
                    .elapsed()
                    .expect("SystemTime error")
                    > WIFI_RETRY_INTERVAL
            {
                info!("Trying to connect to Wi-Fi");
                let mut wifi_option_lock = gs.wifi.lock().unwrap();
                let wifi_lock = wifi_option_lock.as_mut().unwrap();
                match wifi_lock.connect() {
                    Ok(_) => {
                        // Connected
                        wifi_lock.wait_netif_up().unwrap();

                        // Drop the lock
                        drop(wifi_option_lock);

                        // Wait for ESP-NOW to be initialized
                        loop {
                            if gs.esp_now.lock().unwrap().is_some() {
                                break;
                            }
                            thread::sleep(Duration::from_millis(500));
                            // TODO: timelimit
                        }

                        // Acquire the lock again
                        let wifi_option_lock = gs.wifi.lock().unwrap();
                        let wifi_lock = wifi_option_lock.as_ref().unwrap();

                        // Reconfigure broadcast peer channel
                        utilities::espnow::reconfigure_broadcast(wifi_lock);

                        // Drop the lock
                        drop(wifi_option_lock);

                        // Start SNTP service if not already started
                        if gs.sntp.lock().unwrap().is_some() {
                            // Already started
                            continue;
                        }
                        let sntp = sntp::EspSntp::new_default().expect("Failed to initialize SNTP");
                        // Keeping it around or else the SNTP service will stop
                        gs.sntp.lock().unwrap().replace(sntp);

                        continue;
                    }
                    Err(e) => {
                        // Not connected
                        warn!("Failed to connect to Wi-Fi: {:?}", e);
                    }
                }
                last_try_reconnect.replace(SystemTime::now());
            }
        }
    }
}

/// Check if the device is connected to a Wi-Fi network.
pub fn is_connected() -> bool {
    let gs = GlobalState::get();
    let binding = gs.wifi.lock().unwrap();
    let wifi_lock = binding.as_ref();
    // if wifi_lock.is_none() {
    //     return false;
    // }
    // matches!(
    //     wifi_lock.unwrap().get_configuration(),
    //     Ok(wifi::Configuration::Mixed(_, _)) | Ok(wifi::Configuration::Client(_))
    // )
    wifi_lock
        .unwrap()
        .wifi()
        .driver()
        .is_sta_connected()
        .unwrap_or(false)
}
