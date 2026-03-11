use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, MutexGuard, broadcast};

use crate::config::Config;
use crate::dap::client::DapClient;
use crate::dap::state_machine::SessionState;
use crate::dap::types::DapEvent;
use crate::error::AppError;

/// Shared application state, wrapped in `Arc` for concurrent access.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub dap_client: Arc<Mutex<Option<DapClient>>>,
    pub session: Arc<Mutex<SessionState>>,
    pub event_tx: broadcast::Sender<DapEvent>,
    /// Tracks breakpoints per file for DAP's replace-all setBreakpoints semantics.
    pub breakpoints: Arc<Mutex<HashMap<String, Vec<serde_json::Value>>>>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            config: Arc::new(config),
            dap_client: Arc::new(Mutex::new(None)),
            session: Arc::new(Mutex::new(SessionState::new())),
            event_tx,
            breakpoints: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Lock the DAP client, returning an error if no session is active.
    pub async fn require_client(&self) -> Result<MutexGuard<'_, Option<DapClient>>, AppError> {
        let guard = self.dap_client.lock().await;
        if guard.is_none() {
            return Err(AppError::NoSession);
        }
        Ok(guard)
    }

    /// Assert that no session is currently active.
    pub async fn require_no_client(&self) -> Result<(), AppError> {
        let guard = self.dap_client.lock().await;
        if guard.is_some() {
            return Err(AppError::SessionActive);
        }
        drop(guard);
        Ok(())
    }
}
