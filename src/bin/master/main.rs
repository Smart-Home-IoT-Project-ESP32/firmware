#![feature(let_chains)]
use core::time::Duration;
use std::{collections::HashMap, fmt::Write, thread::sleep, time::SystemTime};

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
use firmware::{
    definitions::set_message_device_id,
    utilities::{init::init, sd::SD},
};
use messages::Frame;
use std::thread;
use utilities::{
    constants::{BROADCAST_PING_INTERVAL, SD_RETRY_INTERVAL, SSID, STACK_SIZE},
    espnow::espnow_recv_cb,
    global_state::GlobalState,
    http_server::request_handler_thread,
    tcp_client,
};

use esp_idf_hal::sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
use esp_idf_svc::{
    espnow::{EspNow, BROADCAST},
    http::server::EspHttpServer,
};

use esp_idf_svc::eventloop::*;
use esp_idf_svc::nvs::*;
use esp_idf_svc::wifi::*;
use log::{error, info, warn};

mod utilities;

fn main() {
    // Patches, logger and watchdog reconfiguration
    init();

    // Taking peripherals
    let peripherals = Peripherals::take().unwrap();
    let sys_loop = EspSystemEventLoop::take().unwrap();

    // Init the global state
    let nvs_default_partition = EspDefaultNvsPartition::take().unwrap();
    GlobalState::init(nvs_default_partition.clone());
    let gs = GlobalState::get();

    // Init status LEDs
    // Wi-Fi status LED
    let mut blue_led = PinDriver::output(peripherals.pins.gpio15).unwrap();
    // Blinking when receiving data from the corresponding slaves
    let mut green_led1 = PinDriver::output(peripherals.pins.gpio16).unwrap();
    let mut green_led2 = PinDriver::output(peripherals.pins.gpio17).unwrap();
    let mut green_led3 = PinDriver::output(peripherals.pins.gpio18).unwrap();

    // ----------- //
    // WIFI config //
    // ----------- //
    // Initialize WiFi
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
    let config = if wifi.get_configuration().unwrap() == Configuration::None {
        info!("No WiFi configuration found");
        info!("Creating AP config");
        info!("To configure the device connect to the Wi-Fi network: {:?}. Go to the IP address below and submit the form", SSID);

        // Access Point configuration
        wifi::Configuration::AccessPoint(AccessPointConfiguration {
            ssid: SSID.try_into().unwrap(),
            ssid_hidden: false,
            channel: 0,
            ..Default::default()
        })
    } else {
        let config = wifi.get_configuration().unwrap();
        info!("Found Wi-Fi configuration: {:?}", config);

        config
    };

    // Set the configuration
    wifi.set_configuration(&config).unwrap();

    // Start WiFi
    wifi.start().unwrap();

    // Thread to handle Wi-Fi connection and reconnection
    let _ = thread::Builder::new()
        .name("WiFi connection".to_string())
        .spawn(crate::utilities::wifi::connection_task);

    // Store the wifi in the global state
    gs.wifi.lock().unwrap().replace(wifi);

    // ---------------- //
    // Form page SERVER //
    // ---------------- //
    // HTTP server configuration
    let server_configuration = esp_idf_svc::http::server::Configuration {
        stack_size: STACK_SIZE,
        ..Default::default()
    };

    let mut server = EspHttpServer::new(&server_configuration).unwrap();
    info!("Server created");

    // Channel for communicating the new configs
    let (connection_config_sender, connection_config_receiver) = std::sync::mpsc::sync_channel(10);

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
            utilities::http_server::post_request_handler(req, &connection_config_sender)
        })
        .unwrap();

    // ----------------- //
    // TCP client config //
    // ----------------- //
    // Check for previous TCP server ip
    let mut buffer: [u8; 63] = [0; 63];
    gs.nvs_connect_configs_ns
        .lock()
        .unwrap()
        .get_str("Server IP", &mut buffer)
        .unwrap();

    // If connected to wifi, connect to the TCP server
    if utilities::wifi::is_connected() {
        tcp_client::connect();
    }

    // -------------- //
    // SD card config //
    // -------------- //
    // Initialize SD card
    let spi_config = SpiConfig::new();
    let spi_config = spi_config.baudrate(20.MHz().into());

    let spi = SpiDeviceDriver::new_single(
        peripherals.spi2,
        peripherals.pins.gpio6,
        peripherals.pins.gpio5,
        Some(peripherals.pins.gpio4),
        Option::<AnyIOPin>::None,
        &DriverConfig::default(),
        &spi_config,
    )
    .unwrap();

    let sdmmc_cs = PinDriver::output(peripherals.pins.gpio7).unwrap();
    // Build an SDHandle Card interface out of an SPI device
    let mut spi_device = SdMmcSpi::new(spi, sdmmc_cs);

    let mut sd = SD::new(&mut spi_device).ok();

    // -------------- //
    // ESP-NOW config //
    // -------------- //
    // EspNow start
    let espnow = EspNow::take().unwrap();

    let (tx, rx) = std::sync::mpsc::sync_channel(100);
    espnow
        .register_recv_cb(move |mac_addr, data| espnow_recv_cb(mac_addr, data, &tx))
        .expect("Failed to register receive callback");
    info!("EspNow started");

    // Adding peers in the same channel as the STA
    // To avoid this issue: https://github.com/espressif/esp-idf/issues/10341
    if let Configuration::Mixed(client, _) = gs
        .wifi
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .get_configuration()
        .unwrap()
    {
        // Set the channel
        let channel = client.channel.expect("Channel not set");
        let second = wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
        unsafe {
            esp_idf_hal::sys::esp_wifi_set_channel(channel, second);
            esp_idf_hal::sys::esp_wifi_set_promiscuous(false);
        }

        // Add braodcast peer
        let broadcast = esp_idf_hal::sys::esp_now_peer_info {
            channel,
            ifidx: esp_idf_hal::sys::wifi_interface_t_WIFI_IF_AP,
            encrypt: false,
            peer_addr: BROADCAST,
            ..Default::default()
        };
        espnow.add_peer(broadcast).unwrap();
    }

    // Add espnow to the global state
    gs.esp_now.lock().unwrap().replace(espnow);

    thread::sleep(Duration::from_secs(1));

    // Hash for deserializing the frames from ESP-NOW
    let mut hash = HashMap::new();

    // Spawning a thread to handle the requests from the HTTP server (i.e. the
    // configuration changes).
    let _ = thread::Builder::new()
        .stack_size(5 * 1024)
        .name("Configuration changes handler".to_string())
        .spawn(move || request_handler_thread(connection_config_receiver));

    // --------- //
    // MAIN LOOP //
    // --------- //
    let mut last_broadcast_ts: Option<SystemTime> = None;
    let mut last_sd_retry: Option<SystemTime> = None;
    loop {
        // Sleep for a FreeRTOS tick, this allow the scheduler to run another task
        sleep(Duration::from_millis(10));

        // Brodcast ping message for slaves
        if last_broadcast_ts.is_none()
            || last_broadcast_ts
                .unwrap()
                .elapsed()
                .expect("SystemTime error")
                > BROADCAST_PING_INTERVAL
        {
            if let Some(esp_now) = gs.esp_now.lock().unwrap().as_mut() {
                info!("Broadcasting ping message");
                let message = firmware::PingMessage::new();
                let frame: Frame = message.into();
                if let Err(e) = esp_now.send(BROADCAST, &frame.serialize()) {
                    warn!("Failed to send broadcast ping message: {:?}", e);
                }
            }
            last_broadcast_ts = Some(SystemTime::now());
        }

        // Receive data from the ESP-NOW
        let mut frames_hash = HashMap::new();
        while let Ok((mac_addr, raw_frames)) = rx.try_recv() {
            let vec = hash.entry(mac_addr.clone()).or_insert_with(Vec::new);

            vec.extend_from_slice(raw_frames.as_slice());
            if let Ok(deserialized_frames) = Frame::deserialize_many(vec) {
                let frames_vec = frames_hash.entry(mac_addr).or_insert_with(Vec::new);
                frames_vec.extend(deserialized_frames);
            }
        }

        // Write IDs to each frame
        let mut frames_with_id: Vec<Frame> = Vec::new();
        for (mac_addr, frames) in frames_hash {
            let nvs = gs.nvs_connect_configs_ns.lock().unwrap();
            let mac_addr_str = &mac_addr.iter().fold(String::new(), |mut output, n| {
                let _ = write!(output, "{n:02X}");
                output
            });
            let id = nvs.get_u8(mac_addr_str).unwrap().unwrap_or_else(|| {
                let len = nvs.get_u8("Num of slaves").unwrap().unwrap_or_else(|| {
                    nvs.set_u8("Num of slaves", 0).unwrap();
                    0
                });
                info!("New slave found: {}", mac_addr_str);

                nvs.set_u8(mac_addr_str, len).unwrap();
                info!("Assigned ID: {}", len);

                nvs.set_u8("Num of slaves", len + 1).unwrap();
                info!("New number of slaves: {}", len + 1);
                len
            });

            frames_with_id = frames
                .iter()
                .filter_map(|frame| frame.try_into().ok())
                .map(|mut message| {
                    // Turn on the green LED corresponding to the device ID
                    if id == 0 {
                        green_led1.set_high().unwrap();
                    } else if id == 1 {
                        green_led2.set_high().unwrap();
                    } else if id == 2 {
                        green_led3.set_high().unwrap();
                    }

                    // Set the device ID
                    set_message_device_id(&mut message, id).unwrap();
                    let frame = message.into();

                    // Turn off the green LED corresponding to the device ID
                    if id == 0 {
                        green_led1.set_low().unwrap();
                    } else if id == 1 {
                        green_led2.set_low().unwrap();
                    } else if id == 2 {
                        green_led3.set_low().unwrap();
                    }

                    frame
                })
                .collect();
        }

        // Frames from slaves are timestamped with the time of the reception
        let mut frames_with_id = frames_with_id
            .iter()
            .map(|&x| {
                x.set_timestamp(
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                )
            })
            .collect::<Vec<_>>();

        let mut sent = false;
        if utilities::wifi::is_connected() {
            // If connected to the Wi-Fi, turn on the Wi-Fi status LED
            blue_led.set_high().unwrap();

            // If there is a connection to the Wi-Fi, send the data to the server

            // Extend the frames with the data from the SD card (if any)
            if let Some(mut sd_inner) = sd {
                let sd_frames = sd_inner.read();
                if sd_frames.is_err() {
                    warn!("Failed to read from the SD card");
                }
                frames_with_id.extend(sd_frames.unwrap_or_default());

                sd = Some(sd_inner);
            } else if last_sd_retry.is_none()
                || last_sd_retry.unwrap().elapsed().unwrap() > SD_RETRY_INTERVAL
            {
                warn!("SD card not initialized. Trying to recover...");
                // Try to recover the SD card
                drop(sd);
                sd = SD::new(&mut spi_device).ok();
                last_sd_retry = Some(SystemTime::now());
            }

            // Send the data to the server
            let mut tcp_stream = gs.tcp_stream.lock().unwrap();
            if let Some(stream) = tcp_stream.as_mut() {
                if !frames_with_id.is_empty() {
                    info!("Sending {:#?} to the TCP server", frames_with_id.len());
                }
                for frame in frames_with_id.clone() {
                    //info!("Sending data to the TCP server: {:?}", frame);
                    if let Ok(influx_lp) = frame.to_point() {
                        if let Err(e) = stream.write_point(&influx_lp) {
                            warn!("Failed to send data to the TCP server: {:?}", e);
                        }
                    } else {
                        warn!(
                            "Failed to convert the frame {:?} to InfluxDB line protocol",
                            frame
                        );
                    }
                }
                sent = true;
            } else {
                drop(tcp_stream);

                // TCP was not initialized yet
                tcp_client::connect();
            }
        } else {
            // If not connected to the Wi-Fi, turn off the Wi-Fi status LED
            blue_led.set_low().unwrap();
        }

        if !sent && let Some(mut sd_inner) = sd {
            // There is no connection, store data in the SD card
            for frame in frames_with_id {
                if let Err(err) = sd_inner.write(&frame) {
                    error!("Failed to write to the SD card: {:?}", err);
                }
            }

            sd = Some(sd_inner);
        } else if sd.is_none()
            && (last_sd_retry.is_none()
                || last_sd_retry.unwrap().elapsed().unwrap() > SD_RETRY_INTERVAL)
        {
            warn!("SD card not initialized. Trying to recover...");
            // Try to recover the SD card
            drop(sd);
            sd = SD::new(&mut spi_device).ok();
            last_sd_retry = Some(SystemTime::now());
        }
    }
}
