use std::fmt;

use crate::error::AppError;

/// DAP session lifecycle phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    Uninitialized,
    Initializing,
    Running,
    Stopped,
    Terminated,
}

impl fmt::Display for SessionPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uninitialized => write!(f, "Uninitialized"),
            Self::Initializing => write!(f, "Initializing"),
            Self::Running => write!(f, "Running"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Terminated => write!(f, "Terminated"),
        }
    }
}

/// Tracks the current session phase with validated transitions.
#[derive(Debug)]
pub struct SessionState {
    phase: SessionPhase,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionState {
    pub fn new() -> Self {
        Self {
            phase: SessionPhase::Uninitialized,
        }
    }

    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    /// Attempt a state transition, returning an error for invalid ones.
    pub fn transition(&mut self, to: SessionPhase) -> Result<(), AppError> {
        let valid = matches!(
            (self.phase, to),
            (SessionPhase::Uninitialized, SessionPhase::Initializing)
                | (
                    SessionPhase::Initializing | SessionPhase::Stopped,
                    SessionPhase::Running
                )
                | (
                    SessionPhase::Running,
                    SessionPhase::Stopped | SessionPhase::Terminated
                )
                | (SessionPhase::Stopped, SessionPhase::Terminated)
                | (SessionPhase::Terminated, SessionPhase::Uninitialized)
        );

        if valid {
            self.phase = to;
            Ok(())
        } else {
            Err(AppError::InvalidState {
                from: self.phase.to_string(),
                to: to.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    #[test]
    fn initial_state_is_uninitialized() {
        let state = SessionState::new();
        assert_eq!(state.phase(), SessionPhase::Uninitialized);
    }

    #[test]
    fn valid_transition_uninitialized_to_initializing() {
        let mut state = SessionState::new();
        assert!(state.transition(SessionPhase::Initializing).is_ok());
        assert_eq!(state.phase(), SessionPhase::Initializing);
    }

    #[test]
    fn valid_transition_initializing_to_running() {
        let mut state = SessionState::new();
        state.transition(SessionPhase::Initializing).unwrap();
        assert!(state.transition(SessionPhase::Running).is_ok());
        assert_eq!(state.phase(), SessionPhase::Running);
    }

    #[test]
    fn valid_transition_running_to_stopped() {
        let mut state = SessionState::new();
        state.transition(SessionPhase::Initializing).unwrap();
        state.transition(SessionPhase::Running).unwrap();
        assert!(state.transition(SessionPhase::Stopped).is_ok());
        assert_eq!(state.phase(), SessionPhase::Stopped);
    }

    #[test]
    fn valid_transition_stopped_to_running() {
        let mut state = SessionState::new();
        state.transition(SessionPhase::Initializing).unwrap();
        state.transition(SessionPhase::Running).unwrap();
        state.transition(SessionPhase::Stopped).unwrap();
        assert!(state.transition(SessionPhase::Running).is_ok());
        assert_eq!(state.phase(), SessionPhase::Running);
    }

    #[test]
    fn valid_transition_running_to_terminated() {
        let mut state = SessionState::new();
        state.transition(SessionPhase::Initializing).unwrap();
        state.transition(SessionPhase::Running).unwrap();
        assert!(state.transition(SessionPhase::Terminated).is_ok());
        assert_eq!(state.phase(), SessionPhase::Terminated);
    }

    #[test]
    fn valid_transition_stopped_to_terminated() {
        let mut state = SessionState::new();
        state.transition(SessionPhase::Initializing).unwrap();
        state.transition(SessionPhase::Running).unwrap();
        state.transition(SessionPhase::Stopped).unwrap();
        assert!(state.transition(SessionPhase::Terminated).is_ok());
        assert_eq!(state.phase(), SessionPhase::Terminated);
    }

    #[test]
    fn valid_transition_terminated_to_uninitialized() {
        let mut state = SessionState::new();
        state.transition(SessionPhase::Initializing).unwrap();
        state.transition(SessionPhase::Running).unwrap();
        state.transition(SessionPhase::Terminated).unwrap();
        assert!(state.transition(SessionPhase::Uninitialized).is_ok());
        assert_eq!(state.phase(), SessionPhase::Uninitialized);
    }

    #[test]
    fn full_lifecycle() {
        let mut state = SessionState::new();
        state.transition(SessionPhase::Initializing).unwrap();
        state.transition(SessionPhase::Running).unwrap();
        state.transition(SessionPhase::Stopped).unwrap();
        state.transition(SessionPhase::Running).unwrap();
        state.transition(SessionPhase::Terminated).unwrap();
        state.transition(SessionPhase::Uninitialized).unwrap();
        assert_eq!(state.phase(), SessionPhase::Uninitialized);
    }

    #[test]
    fn invalid_skip_phase() {
        let mut state = SessionState::new();
        let err = state.transition(SessionPhase::Running).unwrap_err();
        assert!(matches!(err, AppError::InvalidState { .. }));
    }

    #[test]
    fn invalid_backwards_transition() {
        let mut state = SessionState::new();
        state.transition(SessionPhase::Initializing).unwrap();
        state.transition(SessionPhase::Running).unwrap();
        let err = state.transition(SessionPhase::Initializing).unwrap_err();
        assert!(matches!(err, AppError::InvalidState { .. }));
    }

    #[test]
    fn invalid_self_transition() {
        let mut state = SessionState::new();
        let err = state.transition(SessionPhase::Uninitialized).unwrap_err();
        assert!(matches!(err, AppError::InvalidState { .. }));
    }

    #[test]
    fn error_contains_state_names() {
        let mut state = SessionState::new();
        let err = state.transition(SessionPhase::Running).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("Uninitialized"),
            "should contain 'Uninitialized': {msg}"
        );
        assert!(msg.contains("Running"), "should contain 'Running': {msg}");
    }
}
