use std::sync::Arc;

use tokio::sync::{Mutex, broadcast};

use crate::config::Config;
use crate::dap::client::DapClient;
use crate::dap::state_machine::SessionState;
use crate::dap::types::DapEvent;

/// Shared application state, wrapped in `Arc` for concurrent access.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub dap_client: Arc<Mutex<Option<DapClient>>>,
    pub session: Arc<Mutex<SessionState>>,
    pub event_tx: broadcast::Sender<DapEvent>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            config: Arc::new(config),
            dap_client: Arc::new(Mutex::new(None)),
            session: Arc::new(Mutex::new(SessionState::new())),
            event_tx,
        }
    }
}
