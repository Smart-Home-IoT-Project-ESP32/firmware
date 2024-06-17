use core::time::Duration;
use std::{net::TcpStream, sync::mpsc::Receiver, thread};

use log::info;

// pub fn tcp_client(init_ip: &str) -> Result<(), std::io::Error> {
//     let mut i = 0;
//     loop {
//         std::io::Write::write_all(
//             &mut stream,
//             format!("weather temperature={}\n", i).as_bytes(),
//         )?;
//         i += 1;
//         thread::sleep(Duration::from_millis(500))
//     }

/*
let mut result = Vec::new();
stream.read_to_end(&mut result)?;
info!(
    "45.79.112.203:4242 returned:\n=================\n{}\n=================\nSince it returned something, all is OK",
    std::str::from_utf8(&result).map_err(|_| io::ErrorKind::InvalidData)?);
*/
// }
