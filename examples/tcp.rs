//! Simple examples of a blocking TCP client communicating with an internet TCP server
//! (google.com) and of a blocking TCP server, that listens for incoming data and echoes it back.

use core::time::Duration;
use std::io::{self, Write};
use std::net::TcpStream;
use std::thread;

use esp_idf_svc::sys::EspError;

/// Set with `export WIFI_SSID=value`.
const SSID: &str = env!("WIFI_SSID");
/// Set with `export WIFI_PASS=value`.
const PASSWORD: &str = env!("WIFI_PASS");

use log::info;

fn main() -> Result<(), anyhow::Error> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Keep it around or else the wifi will stop
    let _wifi = wifi_create()?;

    tcp_client()?;

    Ok(())
}

fn tcp_client() -> Result<(), io::Error> {
    info!("About to open a TCP connection");

    let mut stream = TcpStream::connect("192.168.137.1:8094")?;

    let err = stream.try_clone();
    if let Err(err) = err {
        info!(
            "Duplication of file descriptors does not work (yet) on the ESP-IDF, as expected: {}",
            err
        );
    }

    for i in 0..20 {
        stream.write_all(format!("weather temperature={}\n", i).as_bytes())?;
        thread::sleep(Duration::from_millis(100))
    }

    /*
    let mut result = Vec::new();
    stream.read_to_end(&mut result)?;
    info!(
        "45.79.112.203:4242 returned:\n=================\n{}\n=================\nSince it returned something, all is OK",
        std::str::from_utf8(&result).map_err(|_| io::ErrorKind::InvalidData)?);
    */

    Ok(())
}

fn wifi_create() -> Result<esp_idf_svc::wifi::EspWifi<'static>, EspError> {
    use esp_idf_svc::eventloop::*;
    use esp_idf_svc::hal::prelude::Peripherals;
    use esp_idf_svc::nvs::*;
    use esp_idf_svc::wifi::*;

    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let peripherals = Peripherals::take()?;

    let mut esp_wifi = EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs.clone()))?;
    let mut wifi = BlockingWifi::wrap(&mut esp_wifi, sys_loop.clone())?;

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: PASSWORD.try_into().unwrap(),
        channel: None,
    }))?;

    wifi.start()?;
    info!("Wifi started");

    wifi.connect()?;
    info!("Wifi connected");

    wifi.wait_netif_up()?;
    info!("Wifi netif up");

    Ok(esp_wifi)
}
