use serde::Deserialize;

/// Application configuration with sensible defaults.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Maximum character length for variable values before truncation.
    pub max_variable_length: usize,

    /// Number of source lines to show above/below a breakpoint.
    pub source_context_lines: usize,

    /// Timeout in seconds for DAP requests.
    pub dap_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_variable_length: 1000,
            source_context_lines: 5,
            dap_timeout_secs: 30,
        }
    }
}
