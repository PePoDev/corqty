//! Deployment limits share one bounded parser across API and workers. Constants in
//! graph/runtime remain hard ceilings; operators can lower limits for their resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub nodes: usize,
    pub edges: usize,
    pub run_input_bytes: usize,
    pub node_input_bytes: usize,
    pub node_output_bytes: usize,
    pub log_bytes: usize,
    pub webhook_bytes: usize,
    pub component_bytes: usize,
    pub wasm_memory_bytes: usize,
    pub default_timeout_seconds: u32,
    pub max_timeout_seconds: u32,
}

#[derive(Debug, thiserror::Error)]
#[error("{0} must be a positive whole number within the documented hard ceiling")]
pub struct InvalidLimit(pub &'static str);

impl Default for Limits {
    fn default() -> Self {
        Self {
            nodes: 500,
            edges: 1000,
            run_input_bytes: 1_048_576,
            node_input_bytes: 1_048_576,
            node_output_bytes: 1_048_576,
            log_bytes: 10_485_760,
            webhook_bytes: 1_048_576,
            component_bytes: 26_214_400,
            wasm_memory_bytes: 268_435_456,
            default_timeout_seconds: 900,
            max_timeout_seconds: 14_400,
        }
    }
}

impl Limits {
    pub fn from_env() -> Result<Self, InvalidLimit> {
        Self::parse(|name| std::env::var(name).ok())
    }

    /// Executables validate once at startup; this fallback keeps pure domain callers
    /// deterministic when used without an executable configuration phase.
    pub fn deployment() -> Self {
        Self::from_env().unwrap_or_default()
    }

    pub fn parse(mut get: impl FnMut(&str) -> Option<String>) -> Result<Self, InvalidLimit> {
        let mut read = |name: &'static str, default, max| match get(name) {
            None => Ok(default),
            Some(value) => value
                .parse::<usize>()
                .ok()
                .filter(|value| (1..=max).contains(value))
                .ok_or(InvalidLimit(name)),
        };
        let max_timeout_seconds = read("CORQTY_MAX_TIMEOUT_SECONDS", 14_400, 14_400)? as u32;
        let default_timeout_seconds = read(
            "CORQTY_DEFAULT_TIMEOUT_SECONDS",
            900_u32.min(max_timeout_seconds) as usize,
            max_timeout_seconds as usize,
        )? as u32;
        Ok(Self {
            nodes: read("CORQTY_MAX_NODES", 500, 500)?,
            edges: read("CORQTY_MAX_EDGES", 1000, 1000)?,
            run_input_bytes: read("CORQTY_MAX_RUN_INPUT_BYTES", 1_048_576, 1_048_576)?,
            node_input_bytes: read("CORQTY_MAX_NODE_INPUT_BYTES", 1_048_576, 1_048_576)?,
            node_output_bytes: read("CORQTY_MAX_NODE_OUTPUT_BYTES", 1_048_576, 1_048_576)?,
            log_bytes: read("CORQTY_MAX_LOG_BYTES", 10_485_760, 10_485_760)?,
            webhook_bytes: read("CORQTY_MAX_WEBHOOK_BYTES", 1_048_576, 1_048_576)?,
            component_bytes: read("CORQTY_MAX_COMPONENT_BYTES", 26_214_400, 26_214_400)?,
            wasm_memory_bytes: read("CORQTY_MAX_WASM_MEMORY_BYTES", 268_435_456, 268_435_456)?,
            default_timeout_seconds,
            max_timeout_seconds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_and_bounded_overrides() {
        assert_eq!(Limits::parse(|_| None).unwrap(), Limits::default());
        let limits = Limits::parse(|name| match name {
            "CORQTY_MAX_NODES" => Some("10".into()),
            "CORQTY_MAX_TIMEOUT_SECONDS" => Some("60".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(limits.nodes, 10);
        assert_eq!(limits.default_timeout_seconds, 60);
        for value in ["0", "-1", "501", "abc"] {
            assert!(
                Limits::parse(|name| (name == "CORQTY_MAX_NODES").then(|| value.to_owned()))
                    .is_err()
            );
        }
        for (key, ceiling) in [
            ("CORQTY_MAX_COMPONENT_BYTES", 26_214_400_usize),
            ("CORQTY_MAX_WASM_MEMORY_BYTES", 268_435_456),
        ] {
            let lower = Limits::parse(|name| (name == key).then(|| "65536".to_owned())).unwrap();
            assert!(lower.component_bytes == 65536 || lower.wasm_memory_bytes == 65536);
            assert!(
                Limits::parse(|name| (name == key).then(|| (ceiling + 1).to_string())).is_err()
            );
        }
    }
}
