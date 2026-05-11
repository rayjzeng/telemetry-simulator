use clap::Parser;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use telemetry_simulator::config::Config;
use telemetry_simulator::delivery::MultiProcessGenerator;
use telemetry_simulator::generator::{GeneratorConfig, MessageStream};
use telemetry_simulator::writer;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::net::UnixListener;
use tokio::signal;

#[tokio::main]
async fn main() -> ExitCode {
    let config = Config::parse();

    if let Err(e) = config.validate() {
        eprintln!("Configuration error: {}", e);
        return ExitCode::from(1);
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();

    tokio::spawn(async move {
        let _ = signal::ctrl_c().await;
        shutdown_clone.store(true, Ordering::Relaxed);
    });

    let json_path = config.json_output().cloned();
    let mut json_messages = if json_path.is_some() {
        Some(Vec::new())
    } else {
        None
    };

    let mut output: Box<dyn tokio::io::AsyncWrite + Unpin + Send> = if config.stdout() {
        Box::new(tokio::io::stdout())
    } else if json_path.is_none() {
        // Only use socket if not using stdout and not using json output
        let _ = std::fs::remove_file(&config.socket);

        let listener = match UnixListener::bind(&config.socket) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind socket: {}", e);
                return ExitCode::from(1);
            }
        };

        let accept_future = listener.accept();
        let timeout = tokio::time::sleep(tokio::time::Duration::from_secs(30));
        tokio::pin!(accept_future);
        tokio::pin!(timeout);

        let stream = tokio::select! {
            result = &mut accept_future => {
                match result {
                    Ok((s, _)) => s,
                    Err(e) => {
                        eprintln!("Accept failed: {}", e);
                        return ExitCode::from(1);
                    }
                }
            }
            _ = &mut timeout => {
                eprintln!("Accept timeout (30s)");
                return ExitCode::from(3);
            }
        };

        Box::new(BufWriter::new(stream))
    } else {
        // JSON output mode - use null writer since we collect messages separately
        Box::new(tokio::io::sink())
    };

    let gen_config = GeneratorConfig {
        seed: config.seed,
        event_names: config.get_event_names(),
        session_types: config.get_session_types(),
        interleave_prob: config.interleave_prob,
        mean_delay_ms: config.mean_delay_ms,
        start_time_ns: config.start_time_ms * 1_000_000,
        drop_rate: config.drop_rate,
        num_processes: config.num_processes,
        event_rate: config.event_rate,
        session_gap_mean_ms: config.session_gap_mean_ms,
        duration_mean_ms: config.duration_mean_ms,
        duration_stddev_ms: config.duration_stddev_ms,
    };
    let mut queue = MultiProcessGenerator::new(gen_config);

    let use_stdout = config.stdout();

    // Exit state tracking
    enum RunMode {
        Duration { duration_sec: u64 },
        Count { min_sessions: u64 },
        Indefinite,
    }

    struct ExitState {
        start_time_ns: u64,
        sessions_started: Vec<u64>,
        mode: RunMode,
        draining: bool,
    }

    impl ExitState {
        fn new(
            num_processes: usize,
            duration_sec: u64,
            min_sessions: u64,
            start_time_ns: u64,
        ) -> Self {
            let mode = if duration_sec > 0 {
                RunMode::Duration { duration_sec }
            } else if min_sessions > 0 {
                RunMode::Count { min_sessions }
            } else {
                RunMode::Indefinite
            };

            Self {
                start_time_ns,
                sessions_started: vec![0; num_processes],
                mode,
                draining: false,
            }
        }

        fn record_session_start(&mut self, process_id: u32) {
            self.sessions_started[process_id as usize] += 1;
        }

        fn should_start_draining(&self, current_time_ns: u64) -> bool {
            if self.draining {
                return false;
            }

            match self.mode {
                RunMode::Duration { duration_sec } => {
                    let elapsed_ns = current_time_ns.saturating_sub(self.start_time_ns);
                    elapsed_ns >= duration_sec * 1_000_000_000
                }
                RunMode::Count { min_sessions } => self
                    .sessions_started
                    .iter()
                    .all(|&count| count >= min_sessions),
                RunMode::Indefinite => false,
            }
        }

        fn start_draining(&mut self) {
            self.draining = true;
        }

        fn is_draining(&self) -> bool {
            self.draining
        }
    }

    let mut exit_state = ExitState::new(
        config.num_processes as usize,
        config.duration_sec,
        config.min_sessions,
        config.start_time_ms * 1_000_000,
    );

    loop {
        // Check if we should transition to draining mode
        if exit_state.should_start_draining(queue.current_time_ns()) {
            exit_state.start_draining();
            queue.start_draining();
            match exit_state.mode {
                RunMode::Duration { .. } => {
                    eprintln!("Duration reached, draining pending sessions...")
                }
                RunMode::Count { .. } => {
                    eprintln!("Session count reached, draining pending sessions...")
                }
                RunMode::Indefinite => {}
            }
        }

        // Check for Ctrl+C
        if shutdown.load(Ordering::Relaxed) {
            eprintln!("Shutdown signal received, draining...");
            if !exit_state.is_draining() {
                exit_state.start_draining();
                queue.start_draining();
            }
        }

        // Try to get next message
        match queue.next_message() {
            Some(msg) => {
                // Track session starts for exit condition
                if let telemetry_simulator::message::Message::Session {
                    is_start: true,
                    process_id,
                    ..
                } = &msg
                {
                    exit_state.record_session_start(*process_id);
                }

                // Collect for JSON output if enabled
                if let Some(ref mut messages) = json_messages {
                    messages.push(msg.clone());
                }

                // Write message to output
                if let Err(e) = write_msg(&mut output, &msg, use_stdout).await {
                    eprintln!("Write error: {}", e);
                    return ExitCode::from(4);
                }
            }
            None => {
                if exit_state.is_draining() {
                    // No more messages and we're draining - exit
                    break;
                }
                // No message ready, sleep briefly to avoid busy loop
                tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
            }
        }
    }

    // Final drain (in case we broke due to Ctrl+C)
    if !exit_state.is_draining() {
        queue.start_draining();
    }
    while let Some(msg) = queue.next_message() {
        // Collect for JSON output if enabled
        if let Some(ref mut messages) = json_messages {
            messages.push(msg.clone());
        }

        if let Err(e) = write_msg(&mut output, &msg, use_stdout).await {
            eprintln!("Write error: {}", e);
            return ExitCode::from(4);
        }
    }

    // Write JSON output if enabled
    if let (Some(path), Some(messages)) = (json_path, json_messages) {
        match serde_json::to_string_pretty(&messages) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    eprintln!("Failed to write JSON file: {}", e);
                    return ExitCode::from(1);
                }
                eprintln!("JSON output written to: {}", path);
            }
            Err(e) => {
                eprintln!("Failed to serialize messages to JSON: {}", e);
                return ExitCode::from(1);
            }
        }
    }

    // Print summary
    let elapsed_ns = queue
        .current_time_ns()
        .saturating_sub(exit_state.start_time_ns);
    let elapsed_sec = elapsed_ns / 1_000_000_000;
    eprintln!("Simulation complete:");
    eprintln!("  Duration: {}s (simulated)", elapsed_sec);
    eprintln!("  Sessions per process: {:?}", exit_state.sessions_started);

    let _ = output.flush().await;
    ExitCode::from(0)
}

async fn write_msg<W: tokio::io::AsyncWrite + Unpin>(
    output: &mut W,
    msg: &telemetry_simulator::message::Message,
    use_stdout: bool,
) -> Result<(), std::io::Error> {
    if use_stdout {
        let mut json = serde_json::to_string_pretty(msg).unwrap();
        json.push('\n');
        output.write_all(json.as_bytes()).await
    } else {
        writer::write_message(output, msg).await
    }
}
