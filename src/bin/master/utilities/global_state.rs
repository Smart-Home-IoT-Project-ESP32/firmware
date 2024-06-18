use std::{
    fmt::Debug,
    sync::{atomic::AtomicBool, Arc, Mutex, OnceLock},
};

use esp_idf_svc::{
    espnow::EspNow,
    nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault},
    wifi::{BlockingWifi, EspWifi},
};
use log::info;
use telegraf::Client;

static GLOBAL_STATE: OnceLock<Arc<GlobalState>> = OnceLock::new();

/// Global state of the program.
/// Initialized once at the beginning.
/// Can be accessed from any part of the program.
/// TODO: check se vanno bene i lifetime static qua sotto
pub struct GlobalState {
    pub(crate) nvs_connect_configs_ns: Mutex<EspNvs<NvsDefault>>,
    pub(crate) wifi: Mutex<Option<BlockingWifi<EspWifi<'static>>>>,
    pub(crate) is_connected_to_wifi: AtomicBool,
    pub(crate) esp_now: Mutex<Option<EspNow<'static>>>,
    pub(crate) tcp_stream: Mutex<Option<Client>>,
}

impl Debug for GlobalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalState")
            .field("is_connected_to_wifi", &self.is_connected_to_wifi)
            .finish()
    }
}

impl GlobalState {
    /// Initialize the global state with default values.
    /// Must be initialized only once.
    pub fn init(nvs_partition: EspDefaultNvsPartition) {
        // NVS config
        let namespace = "Connect configs";
        let nvs = match EspNvs::new(nvs_partition, namespace, true) {
            Ok(nvs) => {
                info!("Got namespace {:?} from default partition", namespace);
                nvs
            }
            Err(e) => panic!("Could't get namespace {:?}", e),
        };

        let gs = GlobalState {
            nvs_connect_configs_ns: Mutex::new(nvs),
            wifi: Mutex::new(None),
            is_connected_to_wifi: AtomicBool::new(false),
            esp_now: Mutex::new(None),
            tcp_stream: Mutex::new(None),
        };
        GLOBAL_STATE
            .set(Arc::new(gs))
            .expect("Global state already initialized");
    }

    // TODO: fix the deinit
    // pub fn deinit() {
    //     // Auto drop the global state
    //     let _ = GLOBAL_STATE.take();
    // }

    /// Get the global state.
    pub fn get() -> Arc<GlobalState> {
        GLOBAL_STATE.get().unwrap().clone()
    }
}
