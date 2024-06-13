use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::espnow::PeerInfo;
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};
use firmware::utilities::channel::set_channel;
use std::thread;

use esp_idf_svc::espnow::EspNow;
use esp_idf_svc::wifi::WifiDriver;
use esp_idf_svc::wifi::{ClientConfiguration, Configuration};

// random unicast mac addresses
const TARGET_MAC: [u8; 6] = [0x06, 0xE8, 0xFB, 0x49, 0xB3, 0x78];
const WIFI_CHANNEL: u8 = 6;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();

    // Setup the Wi-Fi driver
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();

    // Create a WifiDriver instance.
    let mut wifi_driver = WifiDriver::new(peripherals.modem, sys_loop, Some(nvs)).unwrap();

    // Set the Wi-Fi configuration as a client
    wifi_driver
        .set_configuration(&Configuration::Client(ClientConfiguration::default()))
        .unwrap();

    // Wi-Fi start
    wifi_driver.start().unwrap();

    set_channel(WIFI_CHANNEL);

    // Start ESP-NOW
    let espnow = EspNow::take().unwrap();

    // Add peer
    espnow
        .add_peer(PeerInfo {
            peer_addr: TARGET_MAC,
            channel: WIFI_CHANNEL,
            ..Default::default()
        })
        .unwrap();

    // when a packet is received, this callback is called
    espnow
        .register_recv_cb(|mac_address, data| {
            println!("[AC] Received data from {:?}: {:?}", mac_address, data);
        })
        .unwrap();

    // when a packet is sent, this callback is called
    espnow
        .register_send_cb(|mac_address, status| {
            println!(
                "[STATION] Data sent to {:?} with status {:?}",
                mac_address, status
            );
        })
        .unwrap();

    loop {
        //let message = serena_messages::PingMessage::default();
        espnow
            .send(TARGET_MAC, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
            .unwrap();
        println!("Sent message");
        thread::sleep(std::time::Duration::from_millis(1000));
    }
}
