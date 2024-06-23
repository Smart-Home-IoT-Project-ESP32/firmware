//! Simple examples of a blocking TCP client communicating with an internet TCP server
//! (google.com) and of a blocking TCP server, that listens for incoming data and echoes it back.

use core::time::Duration;
use std::thread;

use dht_sensor::{dht11, DhtReading as _};
use esp_idf_hal::{delay, gpio};

use esp_idf_svc::eventloop::*;
use esp_idf_svc::hal::prelude::Peripherals;
use esp_idf_svc::nvs::*;
use esp_idf_svc::wifi::*;

/// Set with `export WIFI_SSID=value`.
const SSID: Option<&str> = option_env!("WIFI_SSID");
/// Set with `export WIFI_PASS=value`.
const PASSWORD: Option<&str> = option_env!("WIFI_PASS");

use firmware::{HumidityMessage, TemperatureMessage};
use log::info;
use messages::Frame;
use telegraf::Client;

fn main() -> Result<(), anyhow::Error> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let peripherals = Peripherals::take()?;

    let mut esp_wifi = EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs.clone()))?;
    let mut wifi = BlockingWifi::wrap(&mut esp_wifi, sys_loop.clone())?;

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: SSID.expect("Set a SSID").try_into().unwrap(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: PASSWORD.expect("Set a Password").try_into().unwrap(),
        channel: None,
    }))?;

    wifi.connect()?;
    info!("Wifi connected");

    wifi.wait_netif_up()?;
    info!("Wifi netif up");

    let mut client = Client::new("tcp://4.232.184.193:8094").unwrap();

    let pin = peripherals.pins.gpio5;
    let mut dhtt_pin = gpio::PinDriver::input_output(pin).unwrap();
    dhtt_pin.set_high().unwrap();

    loop {
        if let Ok(reading) = dht11::Reading::read(&mut delay::Ets, &mut dhtt_pin) {
            println!(
                "Temperature: {}°C, Humidity: {}%",
                reading.temperature, reading.relative_humidity
            );
            // convert the reading to a message
            let message_temp =
                TemperatureMessage::new().with_temperature(reading.temperature.try_into().unwrap());
            let frame: Frame = message_temp.into();
            let _ = client.write_point(&frame.to_point().unwrap());

            let message_hum = HumidityMessage::new().with_humidity(reading.relative_humidity);
            let frame: Frame = message_hum.into();
            let _ = client.write_point(&frame.to_point().unwrap());
        }
        thread::sleep(Duration::from_millis(2000))
    }
}
