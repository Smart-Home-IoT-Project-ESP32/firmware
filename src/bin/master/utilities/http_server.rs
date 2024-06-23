use core::time::Duration;
use std::sync::mpsc::SyncSender;
use std::thread;

use anyhow::Error;
use embedded_svc::http::Headers;
use esp_idf_hal::io::{EspIOError, Read, Write};
use esp_idf_svc::http::server::{EspHttpConnection, Request};
use esp_idf_svc::sntp;
use esp_idf_svc::wifi::{self, AccessPointConfiguration, AuthMethod, ClientConfiguration};
use log::info;
use serde::Deserialize;

use crate::utilities;
use crate::utilities::constants::SSID;

/// Max payload length
const MAX_LEN: usize = 128;
/// Include the HTML page
static INDEX_HTML: &str = include_str!("server_page.html");

#[derive(Deserialize)]
/// Input form data structure.
pub struct FormData<'a> {
    wifi_ssid: &'a str,
    wifi_pass: &'a str,
    ip_addr: &'a str,
}

/// Handle the GET request for the index page.
pub fn get_request_handler(req: Request<&mut EspHttpConnection>) -> Result<(), EspIOError> {
    req.into_ok_response()?
        .write_all(INDEX_HTML.as_bytes())
        .map(|_| ())
}

/// Handle the POST request for the form data.
pub fn post_request_handler(
    mut req: Request<&mut EspHttpConnection>,
    connection_config_sender: &SyncSender<(
        heapless::String<32>,
        heapless::String<64>,
        heapless::String<63>,
    )>,
) -> Result<(), Error> {
    let len = req.content_len().unwrap_or(0) as usize;

    if len > MAX_LEN {
        req.into_status_response(413)?
            .write_all("Request too big".as_bytes())?;
        return Ok(());
    }

    let mut buf = vec![0; len];
    req.read_exact(&mut buf)?;
    let mut resp = req.into_ok_response()?;

    if let Ok(form) = serde_json::from_slice::<FormData>(&buf) {
        info!(
            "Wi-Fi SSID: {}, Password: {}, Ip Address: {}",
            form.wifi_ssid, form.wifi_pass, form.ip_addr
        );

        let ssid: heapless::String<32> = form.wifi_ssid.try_into().unwrap();
        let pwd: heapless::String<64> = form.wifi_pass.try_into().unwrap();
        let ip: heapless::String<63> = form.ip_addr.try_into().unwrap();

        connection_config_sender.send((ssid, pwd, ip)).unwrap();
    } else {
        resp.write_all("JSON error".as_bytes())?;
    }

    Ok(())
}

/// Loop that handles the requests from the HTTP server.
pub fn request_handler_thread(
    receiver: std::sync::mpsc::Receiver<(
        heapless::String<32>,
        heapless::String<64>,
        heapless::String<63>,
    )>,
) {
    let gs = crate::utilities::global_state::GlobalState::get();

    info!("HTTP server request handler thread started");
    loop {
        // Sleep for a FreeRTOS tick, this allow the scheduler to run another task
        thread::sleep(Duration::from_millis(10));

        match receiver.try_recv() {
            Ok((ssid, password, new_ip)) => {
                // ---------------- //
                // WIFI reconfigure //
                // ---------------- //
                // Disconnect from the current Wi-Fi
                let mut wifi_option_lock = gs.wifi.lock().unwrap();
                let wifi_lock = wifi_option_lock.as_mut().unwrap();
                let _ = wifi_lock.disconnect();

                info!("Initilizing Wi-Fi with new configuration");

                let new_config = wifi::Configuration::Mixed(
                    ClientConfiguration {
                        ssid,
                        bssid: None,
                        auth_method: AuthMethod::WPA2Personal,
                        password,
                        channel: None,
                    },
                    AccessPointConfiguration {
                        ssid: SSID.try_into().unwrap(),
                        ssid_hidden: false,
                        channel: 0,
                        ..Default::default()
                    },
                );

                wifi_lock.set_configuration(&new_config).unwrap();

                if let Err(e) = wifi_lock.connect() {
                    info!("Failed to connect to Wi-Fi: {:?}", e);
                    continue;
                }
                info!("Connected to Wi-Fi");

                wifi_lock.wait_netif_up().unwrap();

                // Start SNTP service
                let sntp = sntp::EspSntp::new_default().expect("Failed to initialize SNTP");
                info!("SNTP initialized");
                // Keeping it around or else the SNTP service will stop
                gs.sntp.lock().unwrap().replace(sntp);

                // -------------------------- //
                // TCP connection reconfigure //
                // -------------------------- //
                let mut buffer: [u8; 63] = [0; 63];
                gs.nvs_connect_configs_ns
                    .lock()
                    .unwrap()
                    .get_str("Server IP", &mut buffer)
                    .unwrap();
                let old_ip = std::str::from_utf8(&buffer).unwrap();
                if new_ip != old_ip {
                    info!("New IP address: {}", new_ip);
                    gs.nvs_connect_configs_ns
                        .lock()
                        .unwrap()
                        .set_str("Server IP", &new_ip)
                        .unwrap();

                    // Shutdown the previous connection
                    crate::utilities::tcp_client::shutdown();
                    // Connect to the new IP address
                    crate::utilities::tcp_client::connect();
                } else {
                    info!("IP address not changed, still: {}", new_ip);
                }

                // ------- //
                // ESP-NOW //
                // ------- //
                utilities::espnow::reconfigure_broadcast(wifi_lock);
            }
            Err(err) => {
                if let std::sync::mpsc::TryRecvError::Empty = err {
                    continue;
                }
                info!("Error receiving new configuration: {}", err);
            }
        }
    }
}
