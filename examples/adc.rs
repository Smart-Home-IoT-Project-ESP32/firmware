use esp_idf_hal::adc::config::Resolution::Resolution12Bit;
use esp_idf_hal::adc::*;
use esp_idf_hal::gpio::Gpio1;
use log::info;
use std::thread;
use std::time::Duration;

fn main() {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("ADC single shot example.");

    let peripherals = esp_idf_hal::peripherals::Peripherals::take().unwrap();

    // Adc config
    let adc_config = AdcConfig::new()
        .resolution(Resolution12Bit)
        .calibration(true);
    let mut adc = AdcDriver::new(peripherals.adc1, &adc_config).unwrap();

    // Adc pin
    let mut adc_pin: AdcChannelDriver<'_, { attenuation::DB_11 }, Gpio1> =
        AdcChannelDriver::new(peripherals.pins.gpio1).unwrap();

    loop {
        let value = adc.read(&mut adc_pin).unwrap();

        info!("Value: {}", value);
        thread::sleep(Duration::from_millis(500));
    }
}
