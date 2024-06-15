use std::sync::mpsc::SyncSender;

use anyhow::Error;
use embedded_svc::http::Headers;
use esp_idf_hal::io::{EspIOError, Read, Write};
use esp_idf_svc::http::server::{EspHttpConnection, Request};
use log::info;
use serde::Deserialize;

/// Max payload length
const MAX_LEN: usize = 128;
/// Include the HTML page
static INDEX_HTML: &str = include_str!("server_page.html");
/// TODO: remove below const
/// TCP server address (telegraf)
// const TCP_SERVER_ADDR: &str = "192.168.137.1:8094";

#[derive(Deserialize)]
/// Input form data structure
pub struct FormData<'a> {
    wifi_ssid: &'a str,
    wifi_pass: &'a str,
    ip_addr: &'a str,
}

/// Handle the GET request for the index page
pub fn get_request_handler(req: Request<&mut EspHttpConnection>) -> Result<(), EspIOError> {
    req.into_ok_response()?
        .write_all(INDEX_HTML.as_bytes())
        .map(|_| ())
}

/// Handle the POST request for the form data
pub fn post_request_handler(
    mut req: Request<&mut EspHttpConnection>,
    sender: &SyncSender<(
        heapless::String<32>,
        heapless::String<64>,
        heapless::String<63>,
    )>,
) -> Result<(), Error> {
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
        let ip: heapless::String<63> = form.ip_addr.try_into().unwrap();

        sender.send((ssid, pwd, ip)).unwrap();
    } else {
        resp.write_all("JSON error".as_bytes())?;
    }

    Ok(())
}
