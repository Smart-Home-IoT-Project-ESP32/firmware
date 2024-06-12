use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use core::time::Duration;

use dht_sensor::{dht11, DhtReading};
use embedded_svc::wifi::{ClientConfiguration, Configuration};
use esp_idf_hal::{
    delay::{self, BLOCK},
    gpio,
    prelude::*,
    task::notification::Notification,
    task::wait_notification,
};
use esp_idf_svc::espnow::{EspNow, PeerInfo, SendStatus};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::{
    esp_wifi_set_protocol, wifi_interface_t_WIFI_IF_STA, WIFI_PROTOCOL_11B, WIFI_PROTOCOL_11G,
    WIFI_PROTOCOL_11N, WIFI_PROTOCOL_LR,
};
use esp_idf_svc::wifi::WifiDriver;
use firmware::utilities::channel::*;
use firmware::{FireAlarmMessage, HumidityMessage, TemperatureMessage};
use messages::Frame;

static IS_SEARCHING_CHANNEL: AtomicBool = AtomicBool::new(true);

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
    wifi_driver
        .set_configuration(&Configuration::Client(ClientConfiguration::default()))
        .unwrap();

    // Set protocol to accept Long range also
    unsafe {
        esp_wifi_set_protocol(
            wifi_interface_t_WIFI_IF_STA,
            (WIFI_PROTOCOL_11B | WIFI_PROTOCOL_11G | WIFI_PROTOCOL_11N | WIFI_PROTOCOL_LR) as u8,
        );
    }

    // Wi-Fi start
    wifi_driver.start().unwrap();

    // Start ESP-NOW
    let esp_now = EspNow::take().unwrap();

    // Create a channel to communicate between threads
    let (sender, reciver) = std::sync::mpsc::sync_channel(10);
    // Notification to search for a master board on other channels
    let channel_search = Notification::new();
    let channel_search_notifier = channel_search.notifier();

    // register reciving callback, this is used to add the master board to the peer list
    esp_now
        .register_recv_cb(|mac_address, _data| {
            // Convert slice to array
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
        })
        .unwrap();

    // Register the send callback, this is used to detect if the master is not reachable
    let mut num_fail: usize = 0;
    esp_now
        .register_send_cb(|_mac_addres, status| {
            // if a send fails for more than 10 times, start searching for the master board on other channels
            if let SendStatus::SUCCESS = status {
                num_fail = 0;
                IS_SEARCHING_CHANNEL.store(false, Ordering::Relaxed)
            } else {
                num_fail = num_fail.checked_add(1).unwrap_or(0);
            }

            if num_fail > 10 {
                // Safty: we don't call mem::forget on the Notification
                unsafe {
                    channel_search_notifier.notify(core::num::NonZeroU32::new(1).unwrap());
                }
                IS_SEARCHING_CHANNEL.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();

    // Create a thread to read the dhtt sensor
    let sender_dhtt = sender.clone();
    std::thread::spawn(move || {
        loop {
            if let Ok(reading) = dht11::Reading::read(&mut delay::Ets, &mut dhtt_pin) {
                // convert the reading to a message
                let message_temp = TemperatureMessage::new()
                    .with_temperature(reading.temperature.try_into().unwrap());
                let frame: Frame = message_temp.into();
                // send it to the main task
                sender_dhtt.send(frame).unwrap();
                let message_hum = HumidityMessage::new()
                    .with_humidity(reading.relative_humidity);
                let frame: Frame = message_hum.into();
                sender_dhtt.send(frame).unwrap();
            }
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    });

    // Create a task to read the flame sensor
    std::thread::spawn(move || {
        loop {
            let flame = flame_pin.is_high();
            let message = FireAlarmMessage::new().with_fire_alarm(flame);
            // send it to the main task
            sender.send(message.into()).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(15));
        }
    });

    // Scan for an evailable channel
    std::thread::spawn(move || {
        let mut channel = 6;
        loop {
            while IS_SEARCHING_CHANNEL.load(Ordering::Relaxed) {
                set_channel(channel);
                // channels 1, 6 and 11 are the most common channels
                channel = (channel + 5) % 15;
                std::thread::sleep(Duration::from_secs(2));
            }
            // channel found, wait untill a notification is received
            wait_notification(BLOCK);
        }
    });

    // Main task
    loop {
        // Wait for a message
        let frame_to_send = reciver.recv().unwrap();
        println!("Sending message: {:?}", frame_to_send);
        // If the peer exists, send the message
        if let Ok(peer) = esp_now.fetch_peer(true) {
            esp_now
                .send(peer.peer_addr, &frame_to_send.serialize())
                .unwrap();
        }
    }
}
