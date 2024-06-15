use core::{sync::atomic::AtomicBool, time::Duration};
use std::{sync::mpsc::SyncSender, thread::sleep};

use embedded_sdmmc::SdMmcSpi;
use embedded_svc::{
    http::Method,
    wifi::{self, AccessPointConfiguration},
};
use esp_idf_hal::{
    gpio::{AnyIOPin, PinDriver},
    peripherals::Peripherals,
    prelude::*,
    spi::{config::DriverConfig, SpiConfig, SpiDeviceDriver},
};
use firmware::utilities::{init::init, sd::SD};
use messages::Frame;
use std::thread;
use utilities::tcp_client::tcp_client;

use esp_idf_hal::sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
use esp_idf_svc::{espnow::EspNow, http::server::EspHttpServer, wifi::ClientConfiguration};

use esp_idf_svc::eventloop::*;
use esp_idf_svc::nvs::*;
use esp_idf_svc::wifi::*;
use esp_idf_sys::ESP_NOW_MAX_DATA_LEN;
use log::info;

static IS_CONNECTED_TO_WIFI: AtomicBool = AtomicBool::new(false);

mod utilities;

const MAX_DATA_LEN: usize = ESP_NOW_MAX_DATA_LEN as usize;
// Need lots of stack to parse JSON
const STACK_SIZE: usize = 10240;
/// AP SSID
const SSID: &str = "Smart Home Hub";

fn espnow_recv_cb(
    _mac_address: &[u8],
    data: &[u8],
    channel: &SyncSender<heapless::Vec<u8, MAX_DATA_LEN>>,
) {
    let vec_data = heapless::Vec::<u8, MAX_DATA_LEN>::from_slice(data).unwrap();

    channel.send(vec_data).unwrap();
}

fn main() {
    // Patches, logger and watchdog reconfiguration
    init();

    // Taking peripherals
    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take().unwrap();

    // NVS
    let nvs_default_partition = EspDefaultNvsPartition::take().unwrap();
    let namespace = "TCP connection";
    let mut nvs = match EspNvs::new(nvs_default_partition.clone(), namespace, true) {
        Ok(nvs) => {
            info!("Got namespace {:?} from default partition", namespace);
            nvs
        }
        Err(e) => panic!("Could't get namespace {:?}", e),
    };

    // WiFi configuration
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(
            peripherals.modem,
            sys_loop.clone(),
            Some(nvs_default_partition),
        )
        .unwrap(),
        sys_loop,
    )
    .unwrap();

    // Check for previous WiFi configuration
    if wifi.get_configuration().unwrap() == Configuration::None {
        info!("No WiFi configuration found");

        // Access Point configuration
        let wifi_configuration = wifi::Configuration::AccessPoint(AccessPointConfiguration {
            ssid: SSID.try_into().unwrap(),
            ssid_hidden: false,
            channel: 0,
            ..Default::default()
        });

        wifi.set_configuration(&wifi_configuration).unwrap();

        info!("Creating AP");
    } else {
        info!("Recovering WiFi configuration");
    }

    // Start WiFi
    wifi.start().unwrap();
    wifi.wait_netif_up().unwrap();

    // Check for previous TCP ip
    let mut buffer: [u8; 63] = [0; 63];
    nvs.get_str("Server IP", &mut buffer).unwrap();
    let mut ip = std::str::from_utf8(&buffer).unwrap().to_owned();

    // HTTP server configuration
    let server_configuration = esp_idf_svc::http::server::Configuration {
        stack_size: STACK_SIZE,
        ..Default::default()
    };

    let mut server = EspHttpServer::new(&server_configuration).unwrap();

    let (sender, receiver) = std::sync::mpsc::sync_channel(10);

    info!("Server created");

    // HTTP server get requests handler
    server
        .fn_handler(
            "/",
            Method::Get,
            utilities::http_server::get_request_handler,
        )
        .unwrap();

    // HTTP server post requests handler
    server
        .fn_handler::<anyhow::Error, _>("/post", Method::Post, move |req| {
            utilities::http_server::post_request_handler(req, &sender)
        })
        .unwrap();

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
    // let mut esp_wifi =
    //    EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs.clone())).unwrap();
    //let mut wifi = BlockingWifi::wrap(&mut esp_wifi, sys_loop.clone()).unwrap();

    // wifi.set_configuration(&Configuration::Client(ClientConfiguration::default()));

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

    // wifi.start().unwrap();
    // info!("Wifi started");

    // thread::sleep(Duration::from_secs(1));

    // wifi.set_configuration(&Configuration::Client(ClientConfiguration {
    //         ssid: SSID.try_into().unwrap(),
    //         bssid: None,
    //         auth_method: AuthMethod::WPA2Personal,
    //         password: PASSWORD.try_into().unwrap(),
    //         channel: None,
    //     },));
    // wifi.connect().unwrap();
    // info!("Wifi connected");

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
        channel,
        ifidx: esp_idf_hal::sys::wifi_interface_t_WIFI_IF_AP,
        encrypt: false,
        peer_addr: [0x5E, 0xD9, 0x94, 0x27, 0x97, 0x15],
        ..Default::default()
    };
    espnow.add_peer(peer).unwrap();

    let (tx, rx) = std::sync::mpsc::sync_channel(100);
    espnow
        .register_recv_cb(move |mac_address, data| espnow_recv_cb(mac_address, data, &tx))
        .expect("Failed to register receive callback");
    info!("EspNow started");

    thread::sleep(Duration::from_secs(1));

    // Vec for deserializing the frames from ESP-NOW
    let mut vec = Vec::new();

    loop {
        // Sleep for a FreeRTOS tick, this allow the scheduler to run another task
        sleep(Duration::from_millis(10));

        // Check if there is a new configuration
        // TODO: maybe do this every once in a while
        if let Ok((ssid, password, new_ip)) = receiver.try_recv() {
            // FIXME: così ogni volta si deve riscrivere tutta la configurazione (se si
            // vuole cambiare solo l'ip bisogna riscrivere anche ssid e pwd)
            let _ = wifi.disconnect();

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

            wifi.set_configuration(&new_config).unwrap();

            wifi.connect().unwrap();
            info!("Connected to Wi-Fi");

            wifi.wait_netif_up().unwrap();

            // Save the new IP
            nvs.set_str("Server IP", &new_ip).unwrap();
            ip = new_ip.as_str().to_owned();
        }

        if let Some(mut sd_inner) = sd {
            // Receive data from the ESP-NOW
            let mut frames = Vec::new();
            while let Ok(raw_frames) = rx.try_recv() {
                vec.extend_from_slice(raw_frames.as_slice());
                if let Ok(deserialized_frames) = Frame::deserialize_many(&mut vec) {
                    frames.extend(deserialized_frames);
                }
            }

            if IS_CONNECTED_TO_WIFI.load(core::sync::atomic::Ordering::Relaxed) {
                // There is a connection, send data to the server from the SD card
                let _sd_frames = sd_inner.read();

                // TODO: logic
                info!("TCP connection starting...");
                tcp_client(&ip).unwrap();
            } else {
                // There is no connection, store data in the SD card
                for frame in frames {
                    // TODO: what to do if sd is not working?
                    sd_inner.write(&frame).unwrap();
                }
            }

            // TODO: is there a way to make it work without reinitializing the SD card
            // option every time?
            sd = Some(sd_inner);
        } else {
            // panic!("Arrivati");
            // Try to recover the SD card
            drop(sd);
            // TODO: to this less frequently
            sd = SD::new(&mut spi_device).ok();
        }
    }
}
