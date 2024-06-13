pub fn set_channel(channel: u8) {
    unsafe {
        let second = esp_idf_hal::sys::wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
        esp_idf_hal::sys::esp_wifi_set_channel(channel, second);
    }
}
