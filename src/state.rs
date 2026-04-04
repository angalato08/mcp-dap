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

/// State specifically tied to an active debug session.
/// Grouping these ensures atomic cleanup by dropping the whole session object.
pub struct ActiveSession {
    pub client: DapClient,
    pub session: Arc<Mutex<SessionState>>,
    /// Tracks breakpoints per file for DAP's replace-all setBreakpoints semantics.
    pub breakpoints: Mutex<HashMap<String, Vec<TrackedBreakpoint>>>,
    /// Adapter capabilities from the last `initialize` response.
    pub capabilities: Mutex<Option<serde_json::Value>>,
}

/// Shared application state, wrapped in `Arc` for concurrent access.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub active_session: Arc<Mutex<Option<ActiveSession>>>,
    pub event_tx: broadcast::Sender<DapEvent>,
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
            active_session: Arc::new(Mutex::new(None)),
            event_tx,
            pagination_cache: Arc::new(Mutex::new(pagination_cache)),
        }
    }

    /// Assert that the session is in one of the allowed phases.
    pub async fn require_phase(&self, allowed: &[SessionPhase]) -> Result<(), AppError> {
        let guard = self.active_session.lock().await;
        let session_guard = match guard.as_ref() {
            Some(s) => s.session.lock().await,
            None => {
                // If no session exists, treat it as disconnected/none.
                // If "none" is allowed, this might be fine.
                // But usually tools require an active session.
                return Err(AppError::NoSession);
            }
        };

        let current = session_guard.phase();
        if allowed.contains(&current) {
            Ok(())
        } else {
            Err(AppError::InvalidState {
                from: current.to_string(),
                to: format!("one of {allowed:?}"),
            })
        }
    }

    /// Lock the active session, returning an error if no session is active.
    pub async fn require_session(&self) -> Result<MutexGuard<'_, Option<ActiveSession>>, AppError> {
        let guard = self.active_session.lock().await;
        if guard.is_none() {
            return Err(AppError::NoSession);
        }
        Ok(guard)
    }

    /// Assert that no session is currently active.
    pub async fn require_no_session(&self) -> Result<(), AppError> {
        let guard = self.active_session.lock().await;
        if guard.is_some() {
            return Err(AppError::SessionActive);
        }
        drop(guard);
        Ok(())
    }

    /// Wait for a specific DAP event using a provided matcher function, with a timeout.
    pub async fn wait_for_event<F>(
        &self,
        timeout_secs: u64,
        mut matcher: F,
    ) -> Result<DapEvent, AppError>
    where
        F: FnMut(&DapEvent) -> bool,
    {
        let mut rx = self.event_tx.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if matcher(&event) {
                            return Ok(event);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err(AppError::DapError("event channel closed".into()));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        continue;
                    }
                }
            }
        })
        .await
        .map_err(|_| AppError::DapTimeout(timeout_secs))?
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
    pub async fn try_start_session(&self, client: DapClient) -> Result<(), AppError> {
        let mut guard = self.active_session.lock().await;
        if guard.is_some() {
            return Err(AppError::SessionActive);
        }

        let session = ActiveSession {
            client,
            session: Arc::new(Mutex::new(SessionState::new())),
            breakpoints: Mutex::new(HashMap::new()),
            capabilities: Mutex::new(None),
        };

        session
            .session
            .lock()
            .await
            .transition(SessionPhase::Initializing)?;

        *guard = Some(session);
        Ok(())
    }

    /// Force-cleanup all session state: kill adapter, reset phase, clear breakpoints.
    pub async fn force_cleanup(&self) {
        let mut guard = self.active_session.lock().await;
        if let Some(active) = guard.take() {
            // Kill the child process if it exists.
            let child_handle = active.client.child.lock().await.take();
            if let Some(mut child) = child_handle {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        }
        // Dropping 'active' cleans up everything else.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_whitelist(adapters: Vec<&str>) -> AppState {
        let config = Config {
            allowed_adapters: adapters.into_iter().map(String::from).collect(),
            ..Config::default()
        };
        AppState::new(config)
    }

    #[test]
    fn validate_adapter_allows_whitelisted_basename() {
        let state = state_with_whitelist(vec!["codelldb", "dlv"]);
        assert!(state.validate_adapter_path("codelldb").is_ok());
        assert!(state.validate_adapter_path("/usr/bin/codelldb").is_ok());
        assert!(
            state
                .validate_adapter_path("/home/user/.local/bin/dlv")
                .is_ok()
        );
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
