use crate::message::Message;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::BinaryHeap;
use uuid::Uuid;

#[derive(Clone)]
pub struct GeneratorConfig {
    pub seed: Option<u64>,
    pub event_names: Vec<String>,
    pub session_types: Vec<String>,
    pub interleave_prob: f64,
    pub mean_delay_ms: f64,
    pub start_time_ns: u64,
    pub drop_rate: f64,
    pub num_processes: u32,
    pub event_rate: f64,
    pub session_gap_mean_ms: u64,
    pub duration_mean_ms: u64,
    pub duration_stddev_ms: u64,
}

pub trait MessageStream {
    fn next_message(&mut self) -> Option<Message>;
}

// Internal struct for pending session ends
#[derive(Clone, Eq, PartialEq)]
struct PendingSession {
    session_id: String,
    session_type: String,
    time_ns: u64,
    is_start: bool,
}

impl Ord for PendingSession {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse for min-heap
        other.time_ns.cmp(&self.time_ns)
    }
}

impl PartialOrd for PendingSession {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// ProcessSimulator encapsulates state for a single simulated process.
// It generates raw message samples without any delivery semantics.
pub struct ProcessSimulator {
    process_id: u32,
    rng: StdRng,
    sequence_number: u64,
    synthetic_time_ns: u64,
    event_names: Vec<String>,
    event_rate: f64,
    session_gap_mean_ms: u64,
    duration_mean_ms: u64,
    duration_stddev_ms: u64,
    pending_sessions: BinaryHeap<PendingSession>,
}

impl ProcessSimulator {
    pub fn new(
        process_id: u32,
        seed: Option<u64>,
        start_time_ns: u64,
        event_names: Vec<String>,
        session_types: Vec<String>,
        event_rate: f64,
        session_gap_mean_ms: u64,
        duration_mean_ms: u64,
        duration_stddev_ms: u64,
    ) -> Self {
        let mut rng = match seed {
            Some(s) => StdRng::seed_from_u64(s.wrapping_add(process_id as u64)),
            None => StdRng::from_entropy(),
        };

        let mut s = Self {
            process_id,
            rng,
            sequence_number: 0,
            synthetic_time_ns: start_time_ns,
            event_names,
            event_rate,
            session_gap_mean_ms,
            duration_mean_ms,
            duration_stddev_ms,
            pending_sessions: BinaryHeap::new(),
        };
        for session_type in session_types {
            s.push_session_start(&session_type);
        }
        s
    }

    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    pub fn current_time_ns(&self) -> u64 {
        self.synthetic_time_ns
    }

    fn advance_clock(&mut self) -> u64 {
        let interval_ns = (sample_exponential(&mut self.rng, self.event_rate) * 1e9) as u64;
        self.synthetic_time_ns += interval_ns;
        self.synthetic_time_ns
    }

    fn generate_id(&mut self) -> String {
        let bytes: [u8; 16] = self.rng.gen();
        Uuid::from_bytes(bytes).to_string()
    }

    fn next_sequence(&mut self) -> u64 {
        let seq = self.sequence_number;
        self.sequence_number += 1;
        seq
    }

    fn push_session_start(&mut self, session_type: &String) {
        let session_id = self.generate_id();
        let lambda = 1000.0 / (self.session_gap_mean_ms as f64);
        let gap_sec = sample_exponential(&mut self.rng, lambda);
        let next_session_time_ns = self.current_time_ns() + (gap_sec * 1e9) as u64;
        self.pending_sessions.push(PendingSession {
            session_id: session_id,
            session_type: session_type.clone(),
            time_ns: next_session_time_ns,
            is_start: true,
        });
    }

    fn push_session_end(&mut self, session_id: String, session_type: &String) {
        // Calculate session duration using log-uniform distribution
        let duration_ns = self.random_session_duration();
        let next_session_time_ns = self.current_time_ns() + duration_ns;
        self.pending_sessions.push(PendingSession {
            session_id: session_id,
            session_type: session_type.clone(),
            time_ns: next_session_time_ns,
            is_start: false,
        });
    }

    // Generate random session duration using log-uniform distribution
    fn random_session_duration(&mut self) -> u64 {
        // Convert mean/stddev in ms to lognormal parameters μ, σ
        let m = self.duration_mean_ms as f64;
        let s = self.duration_stddev_ms as f64;

        let sigma_squared = (1.0 + (s / m).powi(2)).ln();
        let sigma = sigma_squared.sqrt();
        let mu = m.ln() - sigma_squared / 2.0;

        // Sample from normal distribution using Box-Muller
        let u1 = self.rng.gen_range(f64::MIN_POSITIVE..1.0);
        let u2 = self.rng.gen_range(0.0..1.0);
        let z0 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        let normal_sample = mu + sigma * z0;

        // Convert from lognormal to milliseconds, then to nanoseconds
        let duration_ms = normal_sample.exp().max(1.0);
        (duration_ms as u64) * 1_000_000
    }

    // Check if any pending sessions should end now, and return the session end message if so
    fn check_pending_session(&mut self) -> Option<Message> {
        if let Some(pending) = self.pending_sessions.peek() {
            if pending.time_ns <= self.synthetic_time_ns {
                let pending = self.pending_sessions.pop().unwrap();
                let session_type = pending.session_type.clone();

                // Schedule next session
                if pending.is_start {
                    self.push_session_end(pending.session_id.clone(), &session_type);
                } else {
                    self.push_session_start(&session_type);
                }

                let seq = self.next_sequence();
                return Some(Message::Session {
                    session_id: pending.session_id,
                    session_type: session_type,
                    timestamp_ns: self.synthetic_time_ns,
                    is_start: pending.is_start,
                    version: 1,
                    process_id: self.process_id,
                    sequence_number: seq,
                });
            }
        }
        None
    }

    // Generate next raw message sample from this process
    pub fn next_message(&mut self) -> Option<Message> {
        // First, check if any session messages can be logged
        if let Some(msg) = self.check_pending_session() {
            return Some(msg);
        }

        // Advance clock for new message
        let timestamp_ns = self.advance_clock();

        // Generate event
        let event_name = self.event_names[self.rng.gen_range(0..self.event_names.len())].clone();
        let seq = self.next_sequence();
        return Some(Message::Event {
            event_id: self.generate_id(),
            event_name,
            timestamp_ns,
            version: 1,
            process_id: self.process_id,
            sequence_number: seq,
        });
    }
}

fn sample_exponential(rng: &mut StdRng, rate: f64) -> f64 {
    let mean = 1.0 / rate;
    // Use gen_range to avoid ln(0) which would produce infinity
    let u = rng.gen_range(f64::MIN_POSITIVE..1.0);
    let sample = -mean * u.ln();
    // Clamp to u64::MAX to prevent overflow when casting
    sample.min(u64::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_process(seed: u64, process_id: u32) -> ProcessSimulator {
        ProcessSimulator::new(
            process_id,
            Some(seed),
            0,
            vec!["click".to_string(), "view".to_string()],
            vec!["browsing".to_string(), "checkout".to_string()],
            1.0,
            1000,
            60000,
            120000,
        )
    }

    // Helper to get next message from event-driven simulator
    fn next_message(sim: &mut ProcessSimulator) -> Option<Message> {
        sim.next_message()
    }

    #[test]
    fn test_process_simulator_deterministic() {
        let mut p1 = make_process(42, 0);
        let mut p2 = make_process(42, 0);

        for _ in 0..50 {
            let m1 = next_message(&mut p1);
            let m2 = next_message(&mut p2);
            assert_eq!(m1, m2);
        }
    }

    #[test]
    fn test_process_simulator_isolation() {
        let mut p0 = make_process(42, 0);
        let mut p1 = make_process(42, 1);

        // Same seed but different process IDs should produce different sequences
        // Collect first 100 messages from each process
        let mut messages0 = Vec::new();
        let mut messages1 = Vec::new();

        for _ in 0..100 {
            if messages0.len() < 100 {
                if let Some(msg) = next_message(&mut p0) {
                    messages0.push(msg);
                }
            }
            if messages1.len() < 100 {
                if let Some(msg) = next_message(&mut p1) {
                    messages1.push(msg);
                }
            }
        }

        // The first 100 messages should be collectively different
        assert_ne!(
            messages0, messages1,
            "First 100 messages should be different between processes"
        );
    }

    #[test]
    fn test_sequence_numbers_increment() {
        let mut p = make_process(42, 0);
        let mut last_seq = None;

        for _ in 0..100 {
            if let Some(msg) = next_message(&mut p) {
                let seq = match msg {
                    Message::Event {
                        sequence_number, ..
                    } => sequence_number,
                    Message::Session {
                        sequence_number, ..
                    } => sequence_number,
                };

                if let Some(last) = last_seq {
                    assert_eq!(seq, last + 1);
                }
                last_seq = Some(seq);
            }
        }
    }

    #[test]
    fn test_process_id_in_messages() {
        let mut p = make_process(42, 5);

        for _ in 0..20 {
            if let Some(msg) = next_message(&mut p) {
                let pid = match msg {
                    Message::Event { process_id, .. } => process_id,
                    Message::Session { process_id, .. } => process_id,
                };
                assert_eq!(pid, 5);
            }
        }
    }

    #[test]
    fn test_session_pairs() {
        let mut p = ProcessSimulator::new(
            0,
            Some(42),
            0,
            vec!["click".to_string()],
            vec!["test".to_string()],
            10.0,
            1000,
            10000,
            100000,
        );

        let mut sessions: std::collections::HashMap<String, (u64, bool)> =
            std::collections::HashMap::new();

        let mut has_session = false;
        for _ in 0..500 {
            if let Some(msg) = next_message(&mut p) {
                if let Message::Session {
                    session_id,
                    timestamp_ns,
                    is_start,
                    ..
                } = msg
                {
                    has_session = true;
                    if is_start {
                        sessions.insert(session_id, (timestamp_ns, false));
                    } else {
                        if let Some((start_ts, _)) = sessions.get(&session_id) {
                            assert!(timestamp_ns > *start_ts, "session end before start");
                            sessions.remove(&session_id);
                        }
                    }
                }
            }
        }
        assert!(has_session);
    }

    #[test]
    fn test_clock_advances() {
        let mut p = make_process(42, 0);
        let mut last_ts = 0;

        let mut has_event = false;
        for _ in 0..50 {
            if let Some(msg) = next_message(&mut p) {
                match msg {
                    Message::Event { timestamp_ns, .. } => {
                        has_event = true;
                        let ts = timestamp_ns;
                        assert!(ts > last_ts, "clock did not advance: {} <= {}", ts, last_ts);
                        assert!(ts - last_ts >= 1, "step too small");
                        last_ts = ts;
                    }
                    Message::Session { timestamp_ns, .. } => {
                        // Sessions do not increment the clock
                        let ts = timestamp_ns;
                        last_ts = ts;
                    }
                };
            }
        }
        assert!(has_event);
    }
}
