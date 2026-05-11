use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "telemetry-sim")]
pub struct Config {
    #[arg(short, long, default_value = "/tmp/telemetry.sock")]
    pub socket: String,

    #[arg(long, default_value = "0.0")]
    pub mean_delay_ms: f64,

    #[arg(long)]
    pub seed: Option<u64>,

    #[arg(long, default_value = "0")]
    pub duration_sec: u64,

    #[arg(long, default_value = "0")]
    pub min_sessions: u64,

    #[arg(long, default_value = "1000")]
    pub session_gap_mean_ms: u64,

    #[arg(long, default_value = "2.0")]
    pub event_rate: f64,

    #[arg(long, default_value = "60000")]
    pub duration_mean_ms: u64,

    #[arg(long, default_value = "120000")]
    pub duration_stddev_ms: u64,

    #[arg(long, default_value = "browsing,checkout,search")]
    pub session_types: String,

    #[arg(long, default_value = "click,view,purchase,scroll,hover")]
    pub event_names: String,

    #[arg(long, default_value = "0.0")]
    pub interleave_prob: f64,

    #[arg(long, default_value = "0")]
    pub start_time_ms: u64,

    #[arg(long, default_value = "0.0")]
    pub drop_rate: f64,

    #[arg(long, default_value = "1")]
    pub num_processes: u32,

    #[cfg(feature = "stdout")]
    #[arg(long)]
    stdout: bool,

    #[cfg(feature = "stdout")]
    #[arg(long)]
    pub json: Option<String>,
}

impl Config {
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
        let config = Config::parse_from(["telemetry-sim", "--session-gap-mean-ms=0"]);
        assert!(config.validate().is_err());
    }
}
