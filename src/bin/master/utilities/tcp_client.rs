use log::{info, warn};
use telegraf::Client;

use crate::utilities::constants::TCP_SERVER_ADDR;

/// Connect to the TCP server with the IP address stored in the NVS.
pub fn connect() {
    let gs = crate::utilities::global_state::GlobalState::get();
    let mut buffer: [u8; 63] = [0; 63];
    gs.nvs_connect_configs_ns
        .lock()
        .unwrap()
        .get_str("Server IP", &mut buffer)
        .unwrap();
    let ip = std::str::from_utf8(&buffer).unwrap();

    info!("About to open a TCP connection with ip: {}", ip);
    let mut connect = Client::new(ip);
    if connect.is_err() {
        warn!("Failed to connect to the TCP server: {:?}", connect.err());
        info!(
            "Trying with default TCP server address: {}",
            TCP_SERVER_ADDR
        );
        gs.nvs_connect_configs_ns
            .lock()
            .unwrap()
            .set_str("Server IP", TCP_SERVER_ADDR)
            .unwrap();

        connect = Client::new(TCP_SERVER_ADDR);
    }
    if let Ok(stream) = connect {
        // Save the TCP stream in the global state
        gs.tcp_stream.lock().unwrap().replace(stream);
    } else {
        warn!(
            "Failed to connect to the default TCP server: {:?}",
            connect.err()
        );
    }
}

pub fn shutdown() {
    let gs = crate::utilities::global_state::GlobalState::get();
    // Close the TCP stream by dropping it
    gs.tcp_stream.lock().unwrap().take();
}
