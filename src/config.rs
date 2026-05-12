use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// CLI configuration parser (all fields optional to detect explicit args)
#[derive(Parser, Debug)]
#[command(name = "telemetry-sim")]
struct CliConfig {
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    #[arg(short, long)]
    pub socket: Option<String>,

    #[arg(long)]
    pub mean_delay_ms: Option<f64>,

    #[arg(long)]
    pub seed: Option<u64>,

    #[arg(long)]
    pub duration_sec: Option<u64>,

    #[arg(long)]
    pub min_sessions: Option<u64>,

    #[arg(long)]
    pub session_gap_mean_ms: Option<u64>,

    #[arg(long)]
    pub event_rate: Option<f64>,

    #[arg(long)]
    pub duration_mean_ms: Option<u64>,

    #[arg(long)]
    pub duration_stddev_ms: Option<u64>,

    #[arg(long)]
    pub session_types: Option<String>,

    #[arg(long)]
    pub event_names: Option<String>,

    #[arg(long)]
    pub interleave_prob: Option<f64>,

    #[arg(long)]
    pub start_time_ms: Option<u64>,

    #[arg(long)]
    pub drop_rate: Option<f64>,

    #[arg(long)]
    pub num_processes: Option<u32>,

    #[cfg(feature = "stdout")]
    #[arg(long)]
    pub stdout: Option<bool>,

    #[cfg(feature = "stdout")]
    #[arg(long)]
    pub json: Option<String>,
}

/// File configuration (TOML deserialization)
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    socket: Option<String>,
    mean_delay_ms: Option<f64>,
    seed: Option<u64>,
    duration_sec: Option<u64>,
    min_sessions: Option<u64>,
    session_gap_mean_ms: Option<u64>,
    event_rate: Option<f64>,
    duration_mean_ms: Option<u64>,
    duration_stddev_ms: Option<u64>,
    session_types: Option<String>,
    event_names: Option<String>,
    interleave_prob: Option<f64>,
    start_time_ms: Option<u64>,
    drop_rate: Option<f64>,
    num_processes: Option<u32>,
    stdout: Option<bool>,
    json: Option<String>,
}

impl FileConfig {
    fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                format!("Config file not found: {}", path.display())
            }
            std::io::ErrorKind::PermissionDenied => {
                format!("Permission denied reading config file: {}", path.display())
            }
            std::io::ErrorKind::IsADirectory => {
                format!("Config path is a directory: {}", path.display())
            }
            _ => format!("Failed to read config file {}: {}", path.display(), e),
        })?;

        toml::from_str(&content)
            .map_err(|e| format!("Failed to parse config file {}: {}", path.display(), e))
    }
}

/// Final validated configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub socket: String,
    pub mean_delay_ms: f64,
    pub seed: Option<u64>,
    pub duration_sec: u64,
    pub min_sessions: u64,
    pub session_gap_mean_ms: u64,
    pub event_rate: f64,
    pub duration_mean_ms: u64,
    pub duration_stddev_ms: u64,
    pub session_types: String,
    pub event_names: String,
    pub interleave_prob: f64,
    pub start_time_ms: u64,
    pub drop_rate: f64,
    pub num_processes: u32,

    #[cfg(feature = "stdout")]
    pub stdout: bool,

    #[cfg(feature = "stdout")]
    pub json: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            socket: "/tmp/telemetry.sock".to_string(),
            mean_delay_ms: 0.0,
            seed: None,
            duration_sec: 0,
            min_sessions: 0,
            session_gap_mean_ms: 1000,
            event_rate: 2.0,
            duration_mean_ms: 60000,
            duration_stddev_ms: 120000,
            session_types: "browsing,checkout,search".to_string(),
            event_names: "click,view,purchase,scroll,hover".to_string(),
            interleave_prob: 0.0,
            start_time_ms: 0,
            drop_rate: 0.0,
            num_processes: 1,
            #[cfg(feature = "stdout")]
            stdout: false,
            #[cfg(feature = "stdout")]
            json: None,
        }
    }
}

impl Config {
    /// Load configuration from CLI and optional config file
    pub fn load() -> Result<Self, String> {
        // Parse CLI
        let cli = CliConfig::parse();

        // Load file if specified
        let (file_config, config_dir) = if let Some(ref path) = cli.config {
            let config_path = if path.is_relative() {
                std::env::current_dir()
                    .map_err(|e| format!("Failed to get current directory: {}", e))?
                    .join(path)
            } else {
                path.clone()
            };

            let file = FileConfig::from_file(&config_path)?;
            let dir = config_path.parent().map(|p| p.to_path_buf());
            (Some(file), dir)
        } else {
            (None, None)
        };

        // Merge configurations
        Self::from_cli_and_file(cli, file_config, config_dir.as_deref())
    }

    /// Merge CLI and file configurations with defaults
    fn from_cli_and_file(
        cli: CliConfig,
        file: Option<FileConfig>,
        config_dir: Option<&Path>,
    ) -> Result<Self, String> {
        let defaults = Config::default();
        let file = file.unwrap_or(FileConfig {
            socket: None,
            mean_delay_ms: None,
            seed: None,
            duration_sec: None,
            min_sessions: None,
            session_gap_mean_ms: None,
            event_rate: None,
            duration_mean_ms: None,
            duration_stddev_ms: None,
            session_types: None,
            event_names: None,
            interleave_prob: None,
            start_time_ms: None,
            drop_rate: None,
            num_processes: None,
            stdout: None,
            json: None,
        });

        // Warn about stdout/json in file when feature disabled
        #[cfg(not(feature = "stdout"))]
        if file.stdout.is_some() || file.json.is_some() {
            eprintln!("Warning: stdout/json options in config file ignored (feature not enabled)");
        }

        // Helper to resolve paths relative to config file directory
        let resolve_path = |path_str: String| -> String {
            let path = PathBuf::from(&path_str);
            if path.is_absolute() {
                path_str
            } else if let Some(base) = config_dir {
                base.join(path).to_string_lossy().to_string()
            } else {
                path_str
            }
        };

        let socket = cli
            .socket
            .or(file.socket)
            .map(|s| resolve_path(s))
            .unwrap_or(defaults.socket);

        let mean_delay_ms = cli
            .mean_delay_ms
            .or(file.mean_delay_ms)
            .unwrap_or(defaults.mean_delay_ms);

        let seed = cli.seed.or(file.seed).or(defaults.seed);

        let duration_sec = cli
            .duration_sec
            .or(file.duration_sec)
            .unwrap_or(defaults.duration_sec);

        let min_sessions = cli
            .min_sessions
            .or(file.min_sessions)
            .unwrap_or(defaults.min_sessions);

        let session_gap_mean_ms = cli
            .session_gap_mean_ms
            .or(file.session_gap_mean_ms)
            .unwrap_or(defaults.session_gap_mean_ms);

        let event_rate = cli
            .event_rate
            .or(file.event_rate)
            .unwrap_or(defaults.event_rate);

        let duration_mean_ms = cli
            .duration_mean_ms
            .or(file.duration_mean_ms)
            .unwrap_or(defaults.duration_mean_ms);

        let duration_stddev_ms = cli
            .duration_stddev_ms
            .or(file.duration_stddev_ms)
            .unwrap_or(defaults.duration_stddev_ms);

        let session_types = cli
            .session_types
            .or(file.session_types)
            .unwrap_or(defaults.session_types);

        let event_names = cli
            .event_names
            .or(file.event_names)
            .unwrap_or(defaults.event_names);

        let interleave_prob = cli
            .interleave_prob
            .or(file.interleave_prob)
            .unwrap_or(defaults.interleave_prob);

        let start_time_ms = cli
            .start_time_ms
            .or(file.start_time_ms)
            .unwrap_or(defaults.start_time_ms);

        let drop_rate = cli
            .drop_rate
            .or(file.drop_rate)
            .unwrap_or(defaults.drop_rate);

        let num_processes = cli
            .num_processes
            .or(file.num_processes)
            .unwrap_or(defaults.num_processes);

        #[cfg(feature = "stdout")]
        let stdout = cli.stdout.or(file.stdout).unwrap_or(defaults.stdout);

        #[cfg(feature = "stdout")]
        let json = cli
            .json
            .or(file.json)
            .map(|s| resolve_path(s))
            .or(defaults.json);

        Ok(Config {
            socket,
            mean_delay_ms,
            seed,
            duration_sec,
            min_sessions,
            session_gap_mean_ms,
            event_rate,
            duration_mean_ms,
            duration_stddev_ms,
            session_types,
            event_names,
            interleave_prob,
            start_time_ms,
            drop_rate,
            num_processes,
            #[cfg(feature = "stdout")]
            stdout,
            #[cfg(feature = "stdout")]
            json,
        })
    }

    /// Parse from CLI args (for testing)
    #[cfg(test)]
    pub fn parse_from<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let cli = CliConfig::parse_from(args);
        Self::from_cli_and_file(cli, None, None).expect("Failed to create config from CLI")
    }

    pub fn stdout(&self) -> bool {
        #[cfg(feature = "stdout")]
        {
            self.stdout
        }
        #[cfg(not(feature = "stdout"))]
        {
            false
        }
    }

    pub fn json_output(&self) -> Option<&String> {
        #[cfg(feature = "stdout")]
        {
            self.json.as_ref()
        }
        #[cfg(not(feature = "stdout"))]
        {
            None
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.socket.len() > 107 {
            return Err("Socket path too long (max 107 characters)".to_string());
        }
        if self.get_session_types().is_empty() {
            return Err("At least one session type is required".to_string());
        }
        if self.interleave_prob < 0.0 || self.interleave_prob > 1.0 {
            return Err("Interleave probability must be between 0.0 and 1.0".to_string());
        }

        if self.mean_delay_ms < 0.0 {
            return Err("Mean delay must be >= 0".to_string());
        }
        if self.mean_delay_ms > 1_000_000.0 {
            return Err("Mean delay must be <= 1,000,000 ms".to_string());
        }
        if self.interleave_prob > 0.0 && self.mean_delay_ms == 0.0 {
            eprintln!(
                "Warning: --interleave-prob > 0 but --mean-delay-ms is 0; no reordering will occur"
            );
        }
        if self.drop_rate < 0.0 || self.drop_rate > 1.0 {
            return Err("Drop rate must be between 0.0 and 1.0".to_string());
        }
        if self.num_processes == 0 {
            return Err("Number of processes must be >= 1".to_string());
        }
        if self.num_processes > 1000 {
            return Err("Number of processes must be <= 1000".to_string());
        }
        if self.duration_sec > 0 && self.min_sessions > 0 {
            return Err(
                "Cannot specify both --duration-sec and --min-sessions (mutually exclusive)"
                    .to_string(),
            );
        }
        if self.session_gap_mean_ms <= 0 {
            return Err("Session rate must be > 0".to_string());
        }
        if self.event_rate <= 0.0 {
            return Err("Event rate must be > 0".to_string());
        }
        if self.duration_mean_ms == 0 {
            return Err("Duration mean must be > 0".to_string());
        }
        if self.duration_stddev_ms == 0 {
            return Err("Duration stddev must be > 0".to_string());
        }
        Ok(())
    }

    pub fn get_session_types(&self) -> Vec<String> {
        self.session_types
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn get_event_names(&self) -> Vec<String> {
        self.event_names
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::NamedTempFile;

    #[test]
    fn test_get_session_types_parses_comma_separated() {
        let config = Config::parse_from(["telemetry-sim", "--session-types", "a, b, c"]);
        assert_eq!(config.get_session_types(), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_get_session_types_filters_empty() {
        let config = Config::parse_from(["telemetry-sim", "--session-types", "a,,b"]);
        assert_eq!(config.get_session_types(), vec!["a", "b"]);
    }

    #[test]
    fn test_duration_and_min_sessions_mutually_exclusive() {
        let config = Config::parse_from([
            "telemetry-sim",
            "--duration-sec",
            "10",
            "--min-sessions",
            "5",
        ]);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_session_gap_mean_ms_validation() {
        let config = Config::parse_from(["telemetry-sim", "--session-gap-mean-ms", "0"]);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_defaults() {
        let config = Config::parse_from(["telemetry-sim"]);
        assert_eq!(config.socket, "/tmp/telemetry.sock");
        assert_eq!(config.mean_delay_ms, 0.0);
        assert_eq!(config.num_processes, 1);
    }

    #[test]
    fn test_cli_overrides_defaults() {
        let config = Config::parse_from([
            "telemetry-sim",
            "--socket",
            "/custom.sock",
            "--num-processes",
            "4",
        ]);
        assert_eq!(config.socket, "/custom.sock");
        assert_eq!(config.num_processes, 4);
        // Other values should be defaults
        assert_eq!(config.mean_delay_ms, 0.0);
    }

    #[test]
    fn test_file_config_parsing() {
        let toml_content = r#"
socket = "/tmp/test.sock"
num_processes = 4
mean_delay_ms = 100.0
seed = 42
"#;
        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), toml_content).unwrap();

        let file_config = FileConfig::from_file(temp_file.path()).unwrap();
        assert_eq!(file_config.socket, Some("/tmp/test.sock".to_string()));
        assert_eq!(file_config.num_processes, Some(4));
        assert_eq!(file_config.mean_delay_ms, Some(100.0));
        assert_eq!(file_config.seed, Some(42));
    }

    #[test]
    fn test_file_config_partial() {
        let toml_content = r#"
num_processes = 8
"#;
        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), toml_content).unwrap();

        let file_config = FileConfig::from_file(temp_file.path()).unwrap();
        assert_eq!(file_config.num_processes, Some(8));
        assert_eq!(file_config.socket, None);
    }

    #[test]
    fn test_file_config_empty() {
        let toml_content = r#""#;
        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), toml_content).unwrap();

        let file_config = FileConfig::from_file(temp_file.path()).unwrap();
        assert_eq!(file_config.socket, None);
        assert_eq!(file_config.num_processes, None);
    }

    #[test]
    fn test_file_config_invalid_toml() {
        let toml_content = r#"
socket = "/tmp/test.sock"
invalid = [
"#;
        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), toml_content).unwrap();

        let result = FileConfig::from_file(temp_file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_file_config_unknown_field() {
        let toml_content = r#"
socket = "/tmp/test.sock"
unknown_field = "value"
"#;
        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), toml_content).unwrap();

        let result = FileConfig::from_file(temp_file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_file_and_cli() {
        let toml_content = r#"
socket = "/tmp/file.sock"
num_processes = 4
mean_delay_ms = 100.0
"#;
        let temp_file = NamedTempFile::new().unwrap();
        fs::write(temp_file.path(), toml_content).unwrap();

        let file_config = FileConfig::from_file(temp_file.path()).unwrap();

        // CLI overrides socket, file provides num_processes and mean_delay_ms
        let cli = CliConfig {
            config: None,
            socket: Some("/tmp/cli.sock".to_string()),
            mean_delay_ms: None,
            seed: None,
            duration_sec: None,
            min_sessions: None,
            session_gap_mean_ms: None,
            event_rate: None,
            duration_mean_ms: None,
            duration_stddev_ms: None,
            session_types: None,
            event_names: None,
            interleave_prob: None,
            start_time_ms: None,
            drop_rate: None,
            num_processes: None,
            #[cfg(feature = "stdout")]
            stdout: None,
            #[cfg(feature = "stdout")]
            json: None,
        };

        let config = Config::from_cli_and_file(cli, Some(file_config), None).unwrap();
        assert_eq!(config.socket, "/tmp/cli.sock"); // CLI wins
        assert_eq!(config.num_processes, 4); // File value
        assert_eq!(config.mean_delay_ms, 100.0); // File value
    }

    #[test]
    fn test_merge_defaults() {
        let cli = CliConfig {
            config: None,
            socket: None,
            mean_delay_ms: None,
            seed: None,
            duration_sec: None,
            min_sessions: None,
            session_gap_mean_ms: None,
            event_rate: None,
            duration_mean_ms: None,
            duration_stddev_ms: None,
            session_types: None,
            event_names: None,
            interleave_prob: None,
            start_time_ms: None,
            drop_rate: None,
            num_processes: None,
            #[cfg(feature = "stdout")]
            stdout: None,
            #[cfg(feature = "stdout")]
            json: None,
        };

        let config = Config::from_cli_and_file(cli, None, None).unwrap();
        assert_eq!(config.socket, "/tmp/telemetry.sock");
        assert_eq!(config.num_processes, 1);
    }
}
