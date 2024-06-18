use std::net::TcpStream;

use log::info;

use crate::TCP_SERVER_ADDR;

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
    let mut connect = TcpStream::connect(ip);
    if connect.is_err() {
        info!("Failed to connect to the TCP server: {:?}", connect.err());
        info!(
            "Switching to default TCP server address: {}",
            TCP_SERVER_ADDR
        );

        gs.nvs_connect_configs_ns
            .lock()
            .unwrap()
            .set_str("Server IP", TCP_SERVER_ADDR)
            .unwrap();

        connect = TcpStream::connect(TCP_SERVER_ADDR);
    }
    let stream = connect.unwrap();
    let err = stream.try_clone();
    if let Err(err) = err {
        info!(
            "Duplication of file descriptors does not work (yet) on the ESP-IDF, as expected: {}",
            err
        );
    }

    // Save the TCP stream in the global state
    gs.tcp_stream.lock().unwrap().replace(stream);
}

pub fn shutdown() {
    let gs = crate::utilities::global_state::GlobalState::get();
    // Close the TCP stream by dropping it
    gs.tcp_stream.lock().unwrap().take();
}
