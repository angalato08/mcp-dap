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
                | (SessionPhase::Initializing, SessionPhase::Running)
                | (SessionPhase::Running, SessionPhase::Stopped)
                | (SessionPhase::Stopped, SessionPhase::Running)
                | (SessionPhase::Running, SessionPhase::Terminated)
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
