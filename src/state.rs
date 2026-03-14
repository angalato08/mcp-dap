use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, MutexGuard, broadcast};

use crate::config::Config;
use crate::context::pagination::PaginationCache;
use crate::dap::client::DapClient;
use crate::dap::state_machine::{SessionPhase, SessionState};
use crate::dap::types::DapEvent;
use crate::error::AppError;
use crate::tools::breakpoint::TrackedBreakpoint;

/// Shared application state, wrapped in `Arc` for concurrent access.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub dap_client: Arc<Mutex<Option<DapClient>>>,
    pub session: Arc<Mutex<SessionState>>,
    pub event_tx: broadcast::Sender<DapEvent>,
    /// Tracks breakpoints per file for DAP's replace-all setBreakpoints semantics.
    pub breakpoints: Arc<Mutex<HashMap<String, Vec<TrackedBreakpoint>>>>,
    /// Adapter capabilities from the last `initialize` response.
    pub capabilities: Arc<Mutex<Option<serde_json::Value>>>,
    /// Cache for paginating large debug evaluation results.
    pub pagination_cache: Arc<Mutex<PaginationCache>>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        let pagination_cache = PaginationCache::new(
            config.pagination_cache_max_entries,
            config.pagination_cache_ttl_secs,
        );
        Self {
            config: Arc::new(config),
            dap_client: Arc::new(Mutex::new(None)),
            session: Arc::new(Mutex::new(SessionState::new())),
            event_tx,
            breakpoints: Arc::new(Mutex::new(HashMap::new())),
            capabilities: Arc::new(Mutex::new(None)),
            pagination_cache: Arc::new(Mutex::new(pagination_cache)),
        }
    }

    /// Assert that the session is in one of the allowed phases.
    pub async fn require_phase(&self, allowed: &[SessionPhase]) -> Result<(), AppError> {
        let session = self.session.lock().await;
        let current = session.phase();
        if allowed.contains(&current) {
            Ok(())
        } else {
            Err(AppError::InvalidState {
                from: current.to_string(),
                to: format!("one of {allowed:?}"),
            })
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

    /// Validate that `adapter_path` is on the configured whitelist.
    /// If the whitelist is empty, all adapters are allowed (escape hatch).
    pub fn validate_adapter_path(&self, adapter_path: &str) -> Result<(), AppError> {
        let allowed = &self.config.allowed_adapters;
        if allowed.is_empty() {
            return Ok(());
        }
        if adapter_path.contains("..") {
            return Err(AppError::UnauthorizedAdapter(
                adapter_path.to_string(),
                allowed.join(", "),
            ));
        }
        let basename = std::path::Path::new(adapter_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if allowed.iter().any(|a| a == basename) {
            Ok(())
        } else {
            Err(AppError::UnauthorizedAdapter(
                adapter_path.to_string(),
                allowed.join(", "),
            ))
        }
    }

    /// Atomically guard against concurrent session starts.
    /// Checks no client exists and transitions to Initializing in one critical section.
    pub async fn try_start_session(&self) -> Result<(), AppError> {
        let guard = self.dap_client.lock().await;
        if guard.is_some() {
            return Err(AppError::SessionActive);
        }
        self.session
            .lock()
            .await
            .transition(SessionPhase::Initializing)?;
        drop(guard);
        Ok(())
    }

    /// Force-cleanup all session state: kill adapter, reset phase, clear breakpoints.
    /// Safe to call from any phase — used for error recovery and adapter crash handling.
    pub async fn force_cleanup(&self) {
        // Extract child handle under lock, then release before awaiting kill/wait.
        let child_handle = {
            let guard = self.dap_client.lock().await;
            if let Some(client) = guard.as_ref() {
                client.child.lock().await.take()
            } else {
                None
            }
        };
        // Kill/wait outside the dap_client lock to avoid blocking other tasks.
        if let Some(mut child) = child_handle {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        // Clear the client.
        *self.dap_client.lock().await = None;
        // Reset session state.
        *self.session.lock().await = SessionState::new();
        // Clear breakpoint tracker.
        self.breakpoints.lock().await.clear();
        // Clear cached capabilities.
        *self.capabilities.lock().await = None;
        // NOTE: pagination cache is intentionally NOT cleared here.
        // Tokens remain valid (with their own TTL) even after the debug session ends,
        // so the agent can still page through previously fetched results.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_whitelist(adapters: Vec<&str>) -> AppState {
        let mut config = Config::default();
        config.allowed_adapters = adapters.into_iter().map(String::from).collect();
        AppState::new(config)
    }

    #[test]
    fn validate_adapter_allows_whitelisted_basename() {
        let state = state_with_whitelist(vec!["codelldb", "dlv"]);
        assert!(state.validate_adapter_path("codelldb").is_ok());
        assert!(state.validate_adapter_path("/usr/bin/codelldb").is_ok());
        assert!(state.validate_adapter_path("/home/user/.local/bin/dlv").is_ok());
    }

    #[test]
    fn validate_adapter_rejects_unknown() {
        let state = state_with_whitelist(vec!["codelldb", "dlv"]);
        assert!(state.validate_adapter_path("/usr/bin/evil").is_err());
        assert!(state.validate_adapter_path("bash").is_err());
    }

    #[test]
    fn validate_adapter_rejects_traversal() {
        let state = state_with_whitelist(vec!["codelldb"]);
        assert!(state.validate_adapter_path("../../../bin/sh").is_err());
        assert!(state.validate_adapter_path("/tmp/../bin/codelldb").is_err());
    }

    #[test]
    fn validate_adapter_empty_whitelist_allows_all() {
        let state = state_with_whitelist(vec![]);
        assert!(state.validate_adapter_path("/anything/goes").is_ok());
        assert!(state.validate_adapter_path("../evil").is_ok());
    }
}
