use esp_idf_hal::peripherals::Peripherals;
use esp_idf_svc::{eventloop::EspSystemEventLoop, nvs::EspDefaultNvsPartition};
use std::thread;

use esp_idf_hal::modem;
use esp_idf_hal::sys::{
    esp_wifi_set_protocol, wifi_interface_t_WIFI_IF_STA, EspError, WIFI_PROTOCOL_11B,
    WIFI_PROTOCOL_11G, WIFI_PROTOCOL_11N, WIFI_PROTOCOL_LR,
};
use esp_idf_svc::espnow::{EspNow, PeerInfo, SendStatus};
use esp_idf_svc::wifi::WifiDriver;
use esp_idf_svc::wifi::{ClientConfiguration, Configuration};
use std::collections::VecDeque;
use std::time::Instant;

const WIFI_CHANNEL: u8 = 6;
const ESP_NOW_MAX_DATA_LEN: usize = esp_idf_hal::sys::ESP_NOW_MAX_DATA_LEN as usize;

/// Types of Wi-Fi protocols.
#[derive(PartialEq)]
pub enum Protocol {
    /// 802.11b LR.
    LongRange,
    /// 802.11b/g/n.
    Default,
}

pub struct Wifi<'a, const BUFFER_SIZE: usize = 250> {
    // Wifi driver
    #[allow(dead_code)]
    wifi_driver: WifiDriver<'a>,
    // ESP-NOW driver.
    pub esp_now: EspNow<'a>,
    // Buffer for send function
    buffer: VecDeque<Vec<u8>>,
    // Next data to be sent
    data: heapless::Vec<u8, ESP_NOW_MAX_DATA_LEN>,
    // Timeout for send function
    time_out: Instant,
}

impl<'a, const BUFFER_SIZE: usize> Wifi<'a, BUFFER_SIZE> {
    /// Creates a new Wifi instance.
    pub fn new(
        mac_address: [u8; 6],
        protocol: Protocol,
        modem: modem::Modem,
        nvs: Option<EspDefaultNvsPartition>,
        sys_loop: EspSystemEventLoop,
    ) -> Self {
        // Set the MAC address.
        unsafe {
            esp_idf_hal::sys::esp_base_mac_addr_set(mac_address.as_ptr());
        }

        // Create a WifiDriver instance.
        let mut driver = WifiDriver::new(modem, sys_loop, nvs).unwrap();

        // Wi-Fi interface configuration
        // TODO: Add configuration options.
        let mut config = ClientConfiguration::default();
        println!("Channel: {:?}", config.channel);
        config.channel = Some(WIFI_CHANNEL);
        let station_config = Configuration::Client(config);

        // Set the desired configuration.
        driver.set_configuration(&station_config).unwrap();

        println!("config: {:?}", driver.get_configuration().unwrap());

        // Set the protocol.
        let mut bitmap = (WIFI_PROTOCOL_11B | WIFI_PROTOCOL_11G | WIFI_PROTOCOL_11N) as u8;
        if protocol == Protocol::LongRange {
            bitmap |= WIFI_PROTOCOL_LR as u8;
        }

        unsafe {
            esp_wifi_set_protocol(wifi_interface_t_WIFI_IF_STA, bitmap);
        }

        // Wi-Fi start
        driver.start().unwrap();

        unsafe {
            let channel = WIFI_CHANNEL;
            let second = esp_idf_hal::sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
            esp_idf_hal::sys::esp_wifi_set_channel(channel, second);
        }

        println!(
            "config after start: {:?}",
            driver.get_configuration().unwrap()
        );

        Wifi {
            wifi_driver: driver,
            esp_now: EspNow::take().unwrap(),
            buffer: VecDeque::new(),
            data: heapless::Vec::new(),
            time_out: Instant::now(),
        }
    }

    pub fn default(mac_address: [u8; 6], modem: modem::Modem) -> Self {
        Self::new(
            mac_address,
            Protocol::LongRange,
            modem,
            Some(EspDefaultNvsPartition::take().unwrap()),
            EspSystemEventLoop::take().unwrap(),
        )
    }

    /// Add new peer.
    // TODO: Add encryption option and channel option.
    pub fn add_peer(&self, target_mac: [u8; 6]) -> Result<(), EspError> {
        self.esp_now.add_peer(PeerInfo {
            channel: WIFI_CHANNEL,
            ifidx: wifi_interface_t_WIFI_IF_STA,
            encrypt: false,
            peer_addr: target_mac,
            ..Default::default()
        })
    }

    /// Get the info about a peer with a given MAC address.
    pub fn get_peer_info(&self, mac_address: [u8; 6]) -> Result<PeerInfo, EspError> {
        self.esp_now.get_peer(mac_address)
    }

    /// Delete a peer with a given MAC address.
    pub fn delete_peer(&self, mac_address: [u8; 6]) -> Result<(), EspError> {
        self.esp_now.del_peer(mac_address)
    }

    /// Send raw data to a peer with a given MAC address.
    pub fn send_raw(&self, target_mac_address: [u8; 6], data: &[u8]) -> Result<(), EspError> {
        self.esp_now.send(target_mac_address, data)
    }

    pub fn register_recv_cb(
        &self,
        callback: impl for<'b, 'c> FnMut(&'b [u8], &'c [u8]) + 'static + Send,
    ) {
        self.esp_now.register_recv_cb(callback).unwrap();
    }

    pub fn register_send_cb(
        &self,
        callback: impl for<'b, 'c> FnMut(&'b [u8], SendStatus) + 'static + Send,
    ) {
        self.esp_now.register_send_cb(callback).unwrap();
    }
}

// random unicast mac addresses
// switch for flashing to other device
const BOARD_MAC: [u8; 6] = [0x5E, 0xD9, 0x94, 0x27, 0x97, 0x15];
const TARGET_MAC: [u8; 6] = [0x06, 0xE8, 0xFB, 0x49, 0xB3, 0x78];

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();

    let sys_loop = EspSystemEventLoop::take().unwrap();
    let nvs = EspDefaultNvsPartition::take().unwrap();

    let mut espnow = Wifi::<200>::new(
        BOARD_MAC,
        Protocol::LongRange,
        peripherals.modem,
        Some(nvs),
        sys_loop,
    );

    // Add peer
    espnow.add_peer(TARGET_MAC).unwrap();
    let info = espnow.get_peer_info(TARGET_MAC);
    println!("Peer info: {:?}", info);

    // when a packet is received, this callback is called
    espnow.register_recv_cb(|mac_address, data| {
        println!("[AC] Received data from {:?}: {:?}", mac_address, data);
    });

    // when a packet is sent, this callback is called
    espnow.register_send_cb(|mac_address, status| {
        println!(
            "[STATION] Data sent to {:?} with status {:?}",
            mac_address, status
        );
    });

    loop {
        //let message = serena_messages::PingMessage::default();
        espnow
            .send_raw(TARGET_MAC, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
            .unwrap();
        println!("Sent message");
        thread::sleep(std::time::Duration::from_millis(1000));
    }
}
