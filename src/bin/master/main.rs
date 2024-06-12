// Delete this when a stable version of bms is reached.
#![allow(warnings, unused)]

use core::{sync::atomic::AtomicBool, time::Duration};
use std::thread::sleep;

use embedded_sdmmc::SdMmcSpi;
use esp_idf_hal::{
    gpio::{AnyIOPin, PinDriver},
    peripherals::Peripherals,
    prelude::*,
    spi::{config::DriverConfig, SpiConfig, SpiDeviceDriver},
};
use firmware::utilities::{init::init, sd::SD};
use std::io::{self, Write};
use std::net::TcpStream;
use std::thread;

use esp_idf_hal::sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
use esp_idf_svc::espnow::EspNow;

use esp_idf_svc::eventloop::*;
use esp_idf_svc::nvs::*;
use esp_idf_svc::wifi::*;
use log::info;

static IS_CONNECTED_TO_WIFI: AtomicBool = AtomicBool::new(false);

/// Set with `export WIFI_SSID=value`.
const SSID: Option<&str> = option_env!("WIFI_SSID");
/// Set with `export WIFI_PASS=value`.
const PASSWORD: Option<&str> = option_env!("WIFI_PASS");

/// TCP server address (telegraf)
const TCP_SERVER_ADDR: &str = "192.168.137.1:8094";

fn tcp_client() -> Result<(), io::Error> {
    info!("About to open a TCP connection");

    let mut stream = TcpStream::connect(TCP_SERVER_ADDR)?;

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

fn espnow_recv_cb(mac_address: &[u8], data: &[u8]) {
    println!("mac: {:?}, data: {:?}", mac_address, data);
}

fn main() {
    init();

    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();

    // Taking peripherals
    let peripherals = Peripherals::take().unwrap();

    // Initialize SD card
    let spi_config = SpiConfig::new();
    let spi_config = spi_config.baudrate(20.MHz().into());

    let spi = SpiDeviceDriver::new_single(
        peripherals.spi2,
        peripherals.pins.gpio1,
        peripherals.pins.gpio2,
        Some(peripherals.pins.gpio0),
        Option::<AnyIOPin>::None,
        &DriverConfig::default(),
        &spi_config,
    )
    .unwrap();

    let sdmmc_cs = PinDriver::output(peripherals.pins.gpio3).unwrap();
    // Build an SDHandle Card interface out of an SPI device
    let mut spi_device = SdMmcSpi::new(spi, sdmmc_cs);

    let mut sd = SD::new(&mut spi_device).ok();

    // Initialize the WiFi
    let mut esp_wifi =
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs.clone())).unwrap();
    let mut wifi = BlockingWifi::wrap(&mut esp_wifi, sys_loop.clone()).unwrap();

    wifi.set_configuration(&Configuration::Client(ClientConfiguration::default()));

    // wifi.set_configuration(&Configuration::AccessPoint(
    //     AccessPointConfiguration {
    //         ssid: "iot-device".try_into().unwrap(),
    //         ssid_hidden: false,
    //         channel: 0,
    //         protocols: Protocol::P802D11B
    //             | Protocol::P802D11BG
    //             | Protocol::P802D11BGN
    //             | Protocol::P802D11BGNLR,
    //         ..Default::default()
    //     },
    // ));

    wifi.start().unwrap();
    info!("Wifi started");

    thread::sleep(Duration::from_secs(1));

    // wifi.set_configuration(&Configuration::Client(ClientConfiguration {
    //         ssid: SSID.try_into().unwrap(),
    //         bssid: None,
    //         auth_method: AuthMethod::WPA2Personal,
    //         password: PASSWORD.try_into().unwrap(),
    //         channel: None,
    //     },));
    // wifi.connect().unwrap();
    // info!("Wifi connected");

    unsafe extern "C" fn event_handler(
        event_handler_arg: *mut std::ffi::c_void,
        event_base: *const i8,
        event_id: i32,
        event_data: *mut std::ffi::c_void,
    ) {
        info!("SmartConfig scan done");
        esp_idf_sys::esp_smartconfig_stop();
    }

    unsafe {
        esp_idf_sys::esp_event_handler_register(
            esp_idf_sys::SC_EVENT,
            esp_idf_sys::ESP_EVENT_ANY_ID,
            Some(event_handler),
            std::ptr::null_mut(),
        )
    };

    // SmartConfig WiFi
    let smartconfig_config = esp_idf_sys::smartconfig_start_config_t::default();
    unsafe {
        esp_idf_sys::esp_smartconfig_start(&smartconfig_config);
    }

    let mut now = std::time::Instant::now();
    loop {
        thread::sleep(Duration::from_millis(100));
        if now.elapsed().as_secs() > 1 {
            info!(
                "event scan done: {:?}",
                esp_idf_sys::smartconfig_event_t_SC_EVENT_SCAN_DONE
            );
            info!(
                "event found channel: {:?}",
                esp_idf_sys::smartconfig_event_t_SC_EVENT_FOUND_CHANNEL
            );
            info!(
                "event got ssid pswd: {:?}",
                esp_idf_sys::smartconfig_event_t_SC_EVENT_GOT_SSID_PSWD
            );
            info!(
                "event ack: {:?}",
                esp_idf_sys::smartconfig_event_t_SC_EVENT_SEND_ACK_DONE
            );
            unsafe {
                info!("sc_event: {:?}", esp_idf_sys::SC_EVENT);
            }
            now = std::time::Instant::now();
        }
        // if esp_idf_sys::smartconfig_event_t_SC_EVENT_SCAN_DONE {
        //     info!("SmartConfig scan done");
        //     esp_idf_sys::esp_smartconfig_stop();
        //     break;
        // }
    }

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

    espnow.register_recv_cb(espnow_recv_cb).unwrap();

    thread::sleep(Duration::from_secs(1));

    loop {
        // Sleep for a FreeRTOS tick, this allow the scheduler to run another task
        sleep(Duration::from_millis(10));

        if let Some(mut sd) = sd {
            if IS_CONNECTED_TO_WIFI.load(core::sync::atomic::Ordering::Relaxed) {
                // There is a connection, send data to the server from the SD card
                let frames = sd.read();

                loop {
                    thread::sleep(Duration::from_secs(1));
                }

                tcp_client().unwrap();
            } else {
                // There is no connection, store data in the SD card
                let frame = todo!("get data");
                sd.write(frame);
            }
        } else {
            // Try to recover the SD card
            drop(sd);
            // TODO: to this less frequently
            sd = SD::new(&mut spi_device).ok();
        }
    }
}
