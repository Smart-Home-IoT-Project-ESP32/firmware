use dht_sensor::dht11;
use dht_sensor::DhtReading;
use esp_idf_svc::wifi::WifiDriver;
use esp_idf_svc::espnow::{EspNow, PeerInfo};
use esp_idf_svc::sys::{WIFI_PROTOCOL_LR, WIFI_PROTOCOL_11N, WIFI_PROTOCOL_11G, WIFI_PROTOCOL_11B, wifi_interface_t_WIFI_IF_STA, esp_wifi_set_protocol};
use embedded_svc::wifi::ClientConfiguration;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use embedded_svc::wifi::Configuration;
use esp_idf_hal::delay;
use esp_idf_hal::gpio;
use esp_idf_hal::peripherals::Peripherals;
use firmware::FireAlarmMessage;
use firmware::{TemperatureMessage, HumidityMessage};
use messages::Frame;

const WIFI_CHANNEL: u8 = 1;

fn main() {
    // Init
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Take the peripherals
    let peripherals = Peripherals::take().unwrap();

    // DHT11 sensor
    let mut dhtt_pin = gpio::PinDriver::input_output(peripherals.pins.gpio5).unwrap();
    dhtt_pin.set_high().unwrap();

    // flame sensor
    let flame_pin = gpio::PinDriver::input(peripherals.pins.gpio4).unwrap();

    // ESP-NOW

    // Setup the Wi-Fi driver
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();

    // Create a WifiDriver instance.
    let mut wifi_driver = WifiDriver::new(peripherals.modem, sys_loop, Some(nvs)).unwrap();

    // Set the Wi-Fi configuration as a client
    wifi_driver.set_configuration(&Configuration::Client(ClientConfiguration::default())).unwrap();

    // To avoid this issue: https://github.com/espressif/esp-idf/issues/10341
    unsafe {
        esp_idf_hal::sys::esp_wifi_set_promiscuous(true);
        let channel = WIFI_CHANNEL;
        let second = esp_idf_hal::sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
        esp_idf_hal::sys::esp_wifi_set_channel(channel, second);
        esp_idf_hal::sys::esp_wifi_set_promiscuous(false);
    }

    // Set protocol to accept Long range also
    unsafe {
        esp_wifi_set_protocol(wifi_interface_t_WIFI_IF_STA, (WIFI_PROTOCOL_11B | WIFI_PROTOCOL_11G | WIFI_PROTOCOL_11N | WIFI_PROTOCOL_LR) as u8);
    }

    // Wi-Fi start
    wifi_driver.start().unwrap();

    // Start ESP-NOW
    let esp_now = EspNow::take().unwrap();

    // Create a channel to communicate between threads
    let (sender, reciver) = std::sync::mpsc::sync_channel(10);

    // register reciving callback, this is used to add the master board to the peer list
    esp_now.register_recv_cb(|mac_address, _data| {
        let mac_address_array = mac_address.try_into().unwrap();
        // If peer does not exist, add it
        if let Ok(false) = esp_now.peer_exists(mac_address_array) {
            // Add the peer
            let peer = PeerInfo {
                peer_addr: mac_address_array,
                ..Default::default()
            };
            esp_now.add_peer(peer).unwrap();
        }
    }).unwrap();

    // Create a thread to read the dhtt sensor
    let sender_dhtt = sender.clone();

    std::thread::spawn(move || {
        loop {
            if let Ok(reading) = dht11::Reading::read(&mut delay::Ets, &mut dhtt_pin) {
                // convert the reading to a message
                let message_temp = TemperatureMessage::new().with_temperature(reading.temperature.try_into().unwrap());
                let frame: Frame = message_temp.into();
                // send it to the main task
                sender_dhtt.send(frame).unwrap();
                let message_hum = HumidityMessage::new().with_humidity(reading.relative_humidity.try_into().unwrap());
                let frame: Frame = message_hum.into();
                sender_dhtt.send(frame).unwrap();       
            }
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    });

    // Create a thread to read the flame sensor
    std::thread::spawn(move || {
        loop {
            let flame = flame_pin.is_high();
            let message = FireAlarmMessage::new().with_fire_alarm(flame);
            // send it to the main task
            sender.send(message.into()).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(15));
        }
    });

    // Main task
    loop {
        // Wait for a message
        let frame_to_send = reciver.recv().unwrap();
        // If the peer exists, send the message
        if let Ok(peer) = esp_now.fetch_peer(true) {
            esp_now.send(peer.peer_addr, &frame_to_send.serialize()).unwrap();
        }
    }
}
