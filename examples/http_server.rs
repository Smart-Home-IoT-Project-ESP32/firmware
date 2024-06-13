//! HTTP Server with JSON POST handler
//!
//! Go to 192.168.71.1 to test

use core::convert::TryInto;

use embedded_svc::{
    http::{Headers, Method},
    io::{Read, Write},
    wifi::{self, AccessPointConfiguration},
};

use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    http::server::EspHttpServer,
    nvs::EspDefaultNvsPartition,
    wifi::{AuthMethod, BlockingWifi, EspWifi},
};
use esp_idf_svc::{hal::prelude::Peripherals, wifi::ClientConfiguration};

use log::*;

use serde::Deserialize;

static INDEX_HTML: &str = include_str!("http_server_page.html");

// Max payload length
const MAX_LEN: usize = 128;
// Need lots of stack to parse JSON
const STACK_SIZE: usize = 10240;

#[derive(Deserialize)]
struct FormData<'a> {
    wifi_ssid: &'a str,
    wifi_pass: &'a str,
    ip_addr: &'a str,
}

impl<'a> std::fmt::Display for FormData<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Wi-Fi SSID: {}, Password: {}, Ip Address: {}",
            self.wifi_ssid, self.wifi_pass, self.ip_addr
        )
    }
}

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let (sender, reciver) = std::sync::mpsc::sync_channel(10);
    let (mut server, mut wifi) = create_server()?;

    server.fn_handler("/", Method::Get, |req| {
        req.into_ok_response()?
            .write_all(INDEX_HTML.as_bytes())
            .map(|_| ())
    })?;

    server.fn_handler::<anyhow::Error, _>("/post", Method::Post, move |mut req| {
        let len = req.content_len().unwrap_or(0) as usize;

        if len > MAX_LEN {
            req.into_status_response(413)?
                .write_all("Request too big".as_bytes())?;
            return Ok(());
        }

        let mut buf = vec![0; len];
        req.read_exact(&mut buf)?;
        let mut resp = req.into_ok_response()?;

        if let Ok(form) = serde_json::from_slice::<FormData>(&buf) {
            info!(
                "Wi-Fi SSID: {}, Password: {}, Ip Address: {}",
                form.wifi_ssid, form.wifi_pass, form.ip_addr
            );

            let ssid: heapless::String<32> = form.wifi_ssid.try_into().unwrap();
            let pwd: heapless::String<64> = form.wifi_pass.try_into().unwrap();

            sender.send((ssid, pwd)).unwrap();
        } else {
            resp.write_all("JSON error".as_bytes())?;
        }

        Ok(())
    })?;

    loop {
        let (ssid, pwd) = reciver.recv().unwrap();

        let _ = wifi.disconnect();

        // let config = wifi.get_configuration()?;
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
        wifi.set_configuration(&new_config)?;

        wifi.connect().unwrap();
        info!("Connected to Wi-Fi");

        wifi.wait_netif_up().unwrap();
    }
}

fn create_server() -> anyhow::Result<(EspHttpServer<'static>, BlockingWifi<EspWifi<'static>>)> {
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    let wifi_configuration = wifi::Configuration::AccessPoint(AccessPointConfiguration {
        ssid: "ESP32-Access-Point".try_into().unwrap(),
        ssid_hidden: false,
        channel: 0,
        ..Default::default()
    });

    wifi.set_configuration(&wifi_configuration)?;
    wifi.start()?;
    wifi.wait_netif_up()?;

    info!("Created AP");

    let server_configuration = esp_idf_svc::http::server::Configuration {
        stack_size: STACK_SIZE,
        ..Default::default()
    };

    Ok((EspHttpServer::new(&server_configuration)?, wifi))
}
