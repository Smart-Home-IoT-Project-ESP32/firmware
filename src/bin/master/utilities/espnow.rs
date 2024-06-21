use std::sync::mpsc::SyncSender;

use esp_idf_svc::wifi::{BlockingWifi, Configuration, EspWifi};
use esp_idf_sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
use log::info;

use super::constants::MAX_DATA_LEN;

/// Callback invoked when a frame is received from the ESP-NOW.
/// Sends the received data to the main thread with a channel.
pub fn espnow_recv_cb(
    mac_addr: &[u8],
    data: &[u8],
    channel: &SyncSender<(Vec<u8>, heapless::Vec<u8, MAX_DATA_LEN>)>,
) {
    let vec_data = heapless::Vec::<u8, MAX_DATA_LEN>::from_slice(data).unwrap();

    channel.send((mac_addr.to_vec(), vec_data)).unwrap();
}

/// Reconfigure the broadcast peer to match the WiFI channel.
/// Must be called after a new connection is established.
pub fn reconfigure_broadcast(wifi: &BlockingWifi<EspWifi<'static>>) {
    // Get the global state
    let gs = crate::utilities::global_state::GlobalState::get();

    // Get espnow from global state
    let espnow_option_lock = gs.esp_now.lock().unwrap();
    let espnow = espnow_option_lock
        .as_ref()
        .expect("ESP-NOW not initialized");

    // Get the new channel
    let channel = unsafe {
        esp_idf_hal::sys::esp_wifi_set_promiscuous(true);
        let channel = match wifi.get_configuration().unwrap() {
            Configuration::Mixed(client, _) => client.channel.expect("Channel not set"),
            _ => panic!("Invalid configuration"),
        };
        let second = wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
        esp_idf_hal::sys::esp_wifi_set_channel(channel, second);
        esp_idf_hal::sys::esp_wifi_set_promiscuous(false);
        channel
    };

    info!("New espnow channel: {}", channel);

    // Modify channel in the broadcast peer
    let broadcast = esp_idf_hal::sys::esp_now_peer_info {
        channel,
        ifidx: esp_idf_hal::sys::wifi_interface_t_WIFI_IF_AP,
        encrypt: false,
        peer_addr: [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        ..Default::default()
    };
    // Check if the broadcast is already added
    if espnow.get_peer(broadcast.peer_addr).is_err() {
        espnow.add_peer(broadcast).unwrap();
    } else {
        espnow.mod_peer(broadcast).unwrap();
    }

    // Drop the lock
    drop(espnow_option_lock);
}
