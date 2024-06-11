//! Simple examples of a blocking TCP client communicating with an internet TCP server
//! (google.com) and of a blocking TCP server, that listens for incoming data and echoes it back.

use core::time::Duration;
use std::io::{self, Write};
use std::net::TcpStream;
use std::thread;

use esp_idf_hal::sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
use esp_idf_svc::espnow::EspNow;

use esp_idf_svc::eventloop::*;
use esp_idf_svc::hal::prelude::Peripherals;
use esp_idf_svc::nvs::*;
use esp_idf_svc::wifi::*;

/// Set with `export WIFI_SSID=value`.
const SSID: &str = env!("WIFI_SSID");
/// Set with `export WIFI_PASS=value`.
const PASSWORD: &str = env!("WIFI_PASS");

use log::info;

fn main() -> Result<(), anyhow::Error> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    unsafe {
        esp_idf_hal::sys::esp_base_mac_addr_set([0x06, 0xE8, 0xFB, 0x49, 0xB3, 0x78].as_ptr());
    }   

    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let peripherals = Peripherals::take()?;

    let mut esp_wifi = EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs.clone()))?;
    let mut wifi = BlockingWifi::wrap(&mut esp_wifi, sys_loop.clone())?;

    /*
    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: SSID.try_into().unwrap(),
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: PASSWORD.try_into().unwrap(),
        channel: Some(13),
    }))?;
    */
    
    
    wifi.set_configuration(&Configuration::Mixed(
        ClientConfiguration {
            ssid: SSID.try_into().unwrap(),
            bssid: None,
            auth_method: AuthMethod::WPA2Personal,
            password: PASSWORD.try_into().unwrap(),
            channel: None,
        },
        AccessPointConfiguration {
            ssid: "iot-device".try_into().unwrap(),
            ssid_hidden: false,
            channel: 0,
            protocols: Protocol::P802D11B
                | Protocol::P802D11BG
                | Protocol::P802D11BGN
                | Protocol::P802D11BGNLR,
            ..Default::default()
        },
    ))?;
    
    wifi.start()?;
    info!("Wifi started");

    wifi.connect()?;
    info!("Wifi connected");

    // wifi.wait_netif_up()?;
    info!("Wifi netif up");

    // To avoid this issue: https://github.com/espressif/esp-idf/issues/10341
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

    // EspNow start
    let espnow = EspNow::take().unwrap();

    let peer = esp_idf_hal::sys::esp_now_peer_info {
        channel: channel,
        ifidx: esp_idf_hal::sys::wifi_interface_t_WIFI_IF_AP,
        encrypt: false,
        peer_addr: [0x5E, 0xD9, 0x94, 0x27, 0x97, 0x15],
        ..Default::default()
    };
    espnow.add_peer(peer).unwrap();

    espnow
        .register_recv_cb(|mac_address, data| {
            println!("mac: {:?}, data: {:?}", mac_address, data);
        })
        .unwrap();

    thread::sleep(Duration::from_secs(1));

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

    let mut i = 0;
    loop {
        stream.write_all(format!("weather temperature={}\n", i).as_bytes())?;
        i += 1;
        thread::sleep(Duration::from_millis(500))
    }

    /*
    let mut result = Vec::new();
    stream.read_to_end(&mut result)?;
    info!(
        "45.79.112.203:4242 returned:\n=================\n{}\n=================\nSince it returned something, all is OK",
        std::str::from_utf8(&result).map_err(|_| io::ErrorKind::InvalidData)?);
    */
}
