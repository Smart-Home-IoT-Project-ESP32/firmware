use std::{
    net::TcpStream,
    sync::{Arc, Mutex, OnceLock, RwLock},
};

static GLOBAL_STATE: OnceLock<Arc<GlobalState>> = OnceLock::new();

#[derive(Debug)]
pub struct GlobalState {
    pub(crate) tcp_stream: Mutex<Option<TcpStream>>,
}

impl GlobalState {
    pub fn init() {
        let gs = GlobalState {
            tcp_stream: Mutex::new(None),
        };
        GLOBAL_STATE.set(Arc::new(gs));
    }

    // pub fn deinit() {
    //     // Auto drop the global state
    //     let _ = GLOBAL_STATE.take();
    // }

    pub fn get() -> Arc<GlobalState> {
        GLOBAL_STATE.get().unwrap().clone()
    }
}
