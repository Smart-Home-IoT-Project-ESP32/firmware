use std::sync::atomic::AtomicBool;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};

use anyhow::Error;
use embedded_svc::http::Headers;
use esp_idf_hal::io::{EspIOError, Read, Write};
use esp_idf_svc::espnow::EspNow;
use esp_idf_svc::http::server::{EspHttpConnection, Request};
use esp_idf_svc::nvs::{EspNvs, NvsDefault};
use esp_idf_svc::wifi::{
    self, AccessPointConfiguration, AuthMethod, BlockingWifi, ClientConfiguration, Configuration,
    EspWifi,
};
use esp_idf_sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
use log::info;
use serde::Deserialize;

use crate::SSID;

/// Max payload length
const MAX_LEN: usize = 128;
/// Include the HTML page
static INDEX_HTML: &str = include_str!("server_page.html");
/// TODO: remove below const
/// TCP server address (telegraf)
// const TCP_SERVER_ADDR: &str = "192.168.137.1:8094";

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
    sender: &SyncSender<(
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

        sender.send((ssid, pwd, ip)).unwrap();
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
    wifi: Arc<Mutex<BlockingWifi<EspWifi>>>,
    mut nvs: EspNvs<NvsDefault>,
    ip: std::sync::mpsc::Sender<heapless::String<63>>,
    espnow: &EspNow,
    connected_to_wifi: &AtomicBool,
) {
    loop {
        match receiver.recv() {
            Ok((ssid, password, new_ip)) => {
                // FIXME: così ogni volta si deve riscrivere tutta la configurazione (se si
                // vuole cambiare solo l'ip bisogna riscrivere anche ssid e pwd)

                // Signal disconnection
                connected_to_wifi.store(false, std::sync::atomic::Ordering::Relaxed);
                // Disconnect from the current Wi-Fi
                let mut wifi_lock = wifi.lock().unwrap();
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

                // Save the new IP
                nvs.set_str("Server IP", &new_ip).unwrap();
                ip.send(new_ip).unwrap();

                // ------- //
                // ESP-NOW //
                // ------- //
                // Get the new channel
                let channel = unsafe {
                    esp_idf_hal::sys::esp_wifi_set_promiscuous(true);
                    let channel = match wifi_lock.get_configuration().unwrap() {
                        Configuration::Mixed(client, _) => client.channel.expect("Channel not set"),
                        _ => panic!("Invalid configuration"),
                    };
                    let second = wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
                    esp_idf_hal::sys::esp_wifi_set_channel(channel, second);
                    esp_idf_hal::sys::esp_wifi_set_promiscuous(false);
                    channel
                };
                // Modify channel in peer info
                let peer = esp_idf_hal::sys::esp_now_peer_info {
                    channel,
                    ifidx: esp_idf_hal::sys::wifi_interface_t_WIFI_IF_AP,
                    encrypt: false,
                    peer_addr: [0x5E, 0xD9, 0x94, 0x27, 0x97, 0x15],
                    ..Default::default()
                };
                // Check if the peer is already added
                if espnow.get_peer(peer.peer_addr).is_err() {
                    espnow.add_peer(peer).unwrap();
                } else {
                    espnow.mod_peer(peer).unwrap();
                }

                // Signal connection
                connected_to_wifi.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            Err(err) => {
                info!("Error receiving new configuration: {}", err);
            }
        }
    }
}
