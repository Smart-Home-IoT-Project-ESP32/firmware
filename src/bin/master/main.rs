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
use firmware::utilities::sd::SD;

static IS_CONNECTED_TO_WIFI: AtomicBool = AtomicBool::new(false);

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

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

    loop {
        // Sleep for a FreeRTOS tick, this allow the scheduler to run another task
        sleep(Duration::from_millis(10));

        if let Some(mut sd) = sd {
            if !IS_CONNECTED_TO_WIFI.load(core::sync::atomic::Ordering::Relaxed) {
                // There is a conneciton, send data to the server from the SD card
                let frames = sd.read();
                todo!("Send data to the server")
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
