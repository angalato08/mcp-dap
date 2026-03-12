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

    /// Maximum array items to show when expanding compound values.
    pub max_array_items: usize,

    /// Maximum object keys to show when expanding compound values.
    pub max_object_keys: usize,

    /// Maximum nesting depth for variable/object expansion before truncation.
    pub max_nesting_depth: usize,

    /// Allowed debug adapter basenames (e.g. "codelldb", "debugpy").
    /// Empty list disables the whitelist (allows all adapters).
    pub allowed_adapters: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_variable_length: 1000,
            source_context_lines: 5,
            dap_timeout_secs: 30,
            max_array_items: 10,
            max_object_keys: 10,
            max_nesting_depth: 3,
            allowed_adapters: vec![
                "codelldb".into(),
                "debugpy".into(),
                "dlv".into(),
                "python".into(),
                "python3".into(),
                "node".into(),
                "lldb-dap".into(),
            ],
        }
    }
}
