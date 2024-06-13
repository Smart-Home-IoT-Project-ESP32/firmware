// Slave that is used to read data from LM35 sensor and Gas sensor and send it to the master.
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

use embedded_svc::wifi::ClientConfiguration;
use embedded_svc::wifi::Configuration;

use esp_idf_hal::adc::config::Resolution::Resolution12Bit;
use esp_idf_hal::adc::*;
use esp_idf_hal::delay::BLOCK;
use esp_idf_hal::gpio::Gpio10;
use esp_idf_hal::gpio::Gpio11;
use esp_idf_hal::peripherals::Peripherals;

use esp_idf_hal::task::notification::Notification;
use esp_idf_hal::task::wait_notification;
use esp_idf_svc::espnow::SendStatus;
use esp_idf_svc::espnow::{EspNow, PeerInfo};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::{
    esp_wifi_set_protocol, wifi_interface_t_WIFI_IF_STA, WIFI_PROTOCOL_11B, WIFI_PROTOCOL_11G,
    WIFI_PROTOCOL_11N, WIFI_PROTOCOL_LR,
};
use esp_idf_svc::wifi::WifiDriver;

use firmware::utilities::channel::set_channel;
use firmware::GasLeakageMessage;
use firmware::TemperatureMessage;
use messages::Frame;
use std::thread;
use std::time::Duration;

static IS_SEARCHING_CHANNEL: AtomicBool = AtomicBool::new(true);

fn main() {
    // Init
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Take the peripherals
    let peripherals = Peripherals::take().unwrap();

    // ------------------------------ //
    //       ADC configuration        //
    // ------------------------------ //

    let adc_config = AdcConfig::new()
        .resolution(Resolution12Bit)
        .calibration(true);

    // Create the ADC drivers for the two ADCs
    let mut adc_1: AdcDriver<ADC1> = AdcDriver::new(peripherals.adc1, &adc_config).unwrap();
    let mut adc_2: AdcDriver<ADC2> = AdcDriver::new(peripherals.adc2, &adc_config).unwrap();

    // LM35 sensor (adc_1)
    let mut lm35_temp_pin: AdcChannelDriver<'_, { attenuation::DB_11 }, Gpio10> =
        AdcChannelDriver::new(peripherals.pins.gpio10).unwrap();

    // Gas sensor (adc_2)
    let mut gas_pin: AdcChannelDriver<'_, { attenuation::DB_11 }, Gpio11> =
        AdcChannelDriver::new(peripherals.pins.gpio11).unwrap();

    // ------------------------------ //
    //            ESP-NOW             //
    // ------------------------------ //

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

    // register reciving callback, this is used to add master board to the peer list
    esp_now.register_recv_cb(|mac_address, _data| {
        let mac_address_array = mac_address.try_into().unwrap();
        if let Ok(false) = esp_now.peer_exists(mac_address_array) {
            // Add the peer
            let peer = PeerInfo {
                peer_addr: mac_address_array,
                ..Default::default()
            };
            esp_now.add_peer(peer).unwrap();
        }
    }).unwrap();

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

    // ------------------------------ //
    //            Threads             //
    // ------------------------------ //

    // Create a thread to read the lm35 sensor
    let sender_lm35 = sender.clone();
    std::thread::spawn(move || loop {
        // Read the data from the lm35 sensor using ADC
        let lm35_raw_data = adc_1.read(&mut lm35_temp_pin).unwrap();
        // Convert the raw data to temperature
        let lm35_preprocessed_data = convert_lm35_data(lm35_raw_data);
        // Create a message with the temperature and send it to the main task
        let message_temp =
            TemperatureMessage::new().with_temperature(lm35_preprocessed_data.try_into().unwrap());
        let frame: Frame = message_temp.into();
        sender_lm35.send(frame).unwrap();

        thread::sleep(Duration::from_secs(5));
    });

    // Create a thread to read the gas sensor
    std::thread::spawn(move || loop {
        // Read the data from the gas sensor using ADC
        let gas_data: u16 = adc_2.read(&mut gas_pin).unwrap();
        // Create a message with the gas data and send it to the main task
        let gas_message = GasLeakageMessage::new().with_gas_leakage(gas_data.try_into().unwrap());
        let frame: Frame = gas_message.into();
        sender.send(frame).unwrap();
        thread::sleep(Duration::from_secs(5));
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
        // If a peer is available, send the message
        if let Ok(peer) = esp_now.fetch_peer(true) {
            esp_now
                .send(peer.peer_addr, &frame_to_send.serialize())
                .unwrap();
        }
    }
}

/// Convert the raw data from the LM35 sensor to temperature.
///
/// The `voltage` is calculated by:
/// * multyplying the raw data 3.1, which is the attenuation of the ADC in Volts
/// * and dividing it by 4095, which is the number of bits of the ADC [2^12-1].
///
/// The `temperature` is calculated by multiplying the `voltage` by 100,
/// which is the temperature in Celsius.
///
///
/// # Arguments
///
/// * `raw_data` - The raw data from the LM35 sensor.
///
pub fn convert_lm35_data(raw_data: u16) -> f32 {
    let voltage = raw_data as f32 * 3.1 / 4095.0;
    let temperature = voltage * 100.0;
    temperature
}
