use core::{sync::atomic::AtomicBool, time::Duration};
use std::thread::sleep;

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
use std::net::TcpStream;
use std::thread;

use esp_idf_hal::sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
use esp_idf_svc::{espnow::EspNow, http::server::EspHttpServer, wifi::ClientConfiguration};

use esp_idf_svc::eventloop::*;
use esp_idf_svc::nvs::*;
use esp_idf_svc::wifi::*;
use log::info;

static IS_CONNECTED_TO_WIFI: AtomicBool = AtomicBool::new(false);

mod utilities;

// Need lots of stack to parse JSON
const STACK_SIZE: usize = 10240;

fn tcp_client(ip: &str) -> Result<(), std::io::Error> {
    info!("About to open a TCP connection");

    let mut stream = TcpStream::connect(ip)?;

    let err = stream.try_clone();
    if let Err(err) = err {
        info!(
            "Duplication of file descriptors does not work (yet) on the ESP-IDF, as expected: {}",
            err
        );
    }

    let mut i = 0;
    loop {
        std::io::Write::write_all(
            &mut stream,
            format!("weather temperature={}\n", i).as_bytes(),
        )?;
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
    // Patches, logger and watchdog reconfiguration
    init();

    // Taking peripherals
    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();

    // WiFi configuration
    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs)).unwrap(),
        sys_loop,
    )
    .unwrap();

    // Access Point configuration
    let wifi_configuration = wifi::Configuration::AccessPoint(AccessPointConfiguration {
        ssid: "ESP32-Access-Point".try_into().unwrap(),
        ssid_hidden: false,
        channel: 0,
        ..Default::default()
    });

    wifi.set_configuration(&wifi_configuration).unwrap();
    wifi.start().unwrap();
    wifi.wait_netif_up().unwrap();

    info!("Created AP");

    // HTTP server configuration
    let server_configuration = esp_idf_svc::http::server::Configuration {
        stack_size: STACK_SIZE,
        ..Default::default()
    };

    let mut server = EspHttpServer::new(&server_configuration).unwrap();

    let (sender, reciver) = std::sync::mpsc::sync_channel(10);

    info!("Server created");

    // HTTP server get requests handler
    server
        .fn_handler("/", Method::Get, utilities::server::get_request_handler)
        .unwrap();

    // HTTP server post requests handler
    server
        .fn_handler::<anyhow::Error, _>("/post", Method::Post, move |req| {
            utilities::server::post_request_handler(req, &sender)
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

    // unsafe extern "C" fn event_handler(
    //    event_handler_arg: *mut std::ffi::c_void,
    //    event_base: *const i8,
    //    event_id: i32,
    //    event_data: *mut std::ffi::c_void,
    // ) {
    //    info!("SmartConfig scan done");
    //    esp_idf_sys::esp_smartconfig_stop();
    // }

    // unsafe {
    //    esp_idf_sys::esp_event_handler_register(
    //        esp_idf_sys::SC_EVENT,
    //        esp_idf_sys::ESP_EVENT_ANY_ID,
    //        Some(event_handler),
    //        std::ptr::null_mut(),
    //    )
    //  };

    // SmartConfig WiFi
    // let smartconfig_config = esp_idf_sys::smartconfig_start_config_t::default();
    // unsafe {
    //    esp_idf_sys::esp_smartconfig_start(&smartconfig_config);
    // }

    // let mut now = std::time::Instant::now();
    loop {
        // wifi.wait_netif_up()?;
        // info!("Wifi netif up");

        let (ssid, pwd, ip) = reciver.recv().unwrap();

        let _ = wifi.disconnect();

        info!("Initilizing Wi-Fi");

        let new_config = wifi::Configuration::Mixed(
            ClientConfiguration {
                ssid,
                bssid: None,
                auth_method: AuthMethod::WPA2Personal,
                password: pwd,
                channel: None,
            },
            AccessPointConfiguration {
                ssid: "ESP32-Access-Point".try_into().unwrap(),
                ssid_hidden: false,
                channel: 0,
                ..Default::default()
            },
        );

        wifi.set_configuration(&new_config).unwrap();

        wifi.connect().unwrap();
        info!("Connected to Wi-Fi");

        wifi.wait_netif_up().unwrap();

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

        info!("EspNow started");

        thread::sleep(Duration::from_secs(1));

        loop {
            // Sleep for a FreeRTOS tick, this allow the scheduler to run another task
            sleep(Duration::from_millis(10));

            if let Some(mut sd_inner) = sd {
                if IS_CONNECTED_TO_WIFI.load(core::sync::atomic::Ordering::Relaxed) {
                    // There is a connection, send data to the server from the SD card
                    let _frames = sd_inner.read();

                    // TODO: logic
                    info!("TCP connection starting...");
                    tcp_client(&ip).unwrap();
                } else {
                    // There is no connection, store data in the SD card
                    let frame = todo!("get data");
                    // TODO: what to do if sd is not working?
                    sd_inner.write(frame).unwrap();
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
}
