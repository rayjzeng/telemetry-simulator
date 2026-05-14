use crate::generator::{GeneratorConfig, MessageStream, ProcessSimulator};
use crate::message::Message;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::BinaryHeap;

// Wrapper for delivery-time ordering in the min-heap
#[allow(dead_code)]
struct PendingDelivery {
    delivery_time_ns: u64,
    event_time_ns: u64,
    message: Option<Message>, // None indicates dropped message
    process_id: u32,
    sequence_number: u64,
}

impl PartialEq for PendingDelivery {
    fn eq(&self, other: &Self) -> bool {
        self.delivery_time_ns == other.delivery_time_ns && self.event_time_ns == other.event_time_ns
    }
}

impl Eq for PendingDelivery {}

impl PartialOrd for PendingDelivery {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PendingDelivery {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse for min-heap (earliest delivery first)
        other
            .delivery_time_ns
            .cmp(&self.delivery_time_ns)
            .then(other.event_time_ns.cmp(&self.event_time_ns))
    }
}

pub struct MultiProcessGenerator {
    processes: Vec<ProcessSimulator>,
    process_selector_rng: StdRng,
    drop_rate: f64,
    drop_rng: StdRng,
    interleave_prob: f64,
    mean_delay_ns: f64,
    delivery_heap: BinaryHeap<PendingDelivery>,
    draining: bool,
}

impl MultiProcessGenerator {
    pub fn current_time_ns(&self) -> u64 {
        // Return the maximum time across all processes
        // This ensures we don't stop until all processes have reached duration
        self.processes
            .iter()
            .map(|p| p.current_time_ns())
            .max()
            .unwrap_or(0)
    }

    pub fn new(config: GeneratorConfig) -> Self {
        let processes: Vec<ProcessSimulator> = (0..config.num_processes)
            .map(|pid| {
                ProcessSimulator::new(
                    pid,
                    config.seed,
                    config.start_time_ns,
                    config.event_names.clone(),
                    config.session_types.clone(),
                    config.event_rate,
                    config.session_gap_mean_ms,
                    config.duration_mean_ms,
                    config.duration_stddev_ms,
                )
            })
            .collect();

        let process_selector_rng = match config.seed {
            Some(s) => StdRng::seed_from_u64(s.wrapping_add(0x10000)),
            None => StdRng::from_entropy(),
        };

        let drop_rng = match config.seed {
            Some(s) => StdRng::seed_from_u64(s.wrapping_add(0x20000)),
            None => StdRng::from_entropy(),
        };

        Self {
            processes,
            process_selector_rng,
            drop_rate: config.drop_rate,
            drop_rng,
            interleave_prob: config.interleave_prob,
            mean_delay_ns: config.mean_delay_ms * 1_000_000.0,
            delivery_heap: BinaryHeap::new(),
            draining: false,
        }
    }

    pub fn start_draining(&mut self) {
        for process in &mut self.processes {
            let drain_msgs = process.drain();
            for msg in drain_msgs {
                let event_time_ns = match &msg {
                    Message::Event { timestamp_ns, .. } | Message::Session { timestamp_ns, .. } => {
                        *timestamp_ns
                    }
                };
                let process_id = process.process_id();
                let sequence_number = match &msg {
                    Message::Event {
                        sequence_number, ..
                    }
                    | Message::Session {
                        sequence_number, ..
                    } => *sequence_number,
                };
                self.delivery_heap.push(PendingDelivery {
                    delivery_time_ns: event_time_ns,
                    event_time_ns,
                    message: Some(msg),
                    process_id,
                    sequence_number,
                });
            }
        }
        self.draining = true;
    }

    fn maybe_drop(&mut self, msg: Message) -> Option<Message> {
        if self.drop_rate > 0.0 && self.drop_rng.gen_bool(self.drop_rate) {
            None
        } else {
            Some(msg)
        }
    }

    fn enqueue(&mut self, msg_opt: Option<Message>, process_id: u32, event_time_ns: u64) {
        let sequence_number = msg_opt
            .as_ref()
            .map(|m| match m {
                Message::Event {
                    sequence_number, ..
                } => *sequence_number,
                Message::Session {
                    sequence_number, ..
                } => *sequence_number,
            })
            .unwrap_or(0);

        let delivery_time_ns = if self.interleave_prob > 0.0
            && self.process_selector_rng.gen_bool(self.interleave_prob)
        {
            let delay =
                sample_exponential(&mut self.process_selector_rng, self.mean_delay_ns) as u64;
            event_time_ns + delay
        } else {
            event_time_ns
        };

        self.delivery_heap.push(PendingDelivery {
            delivery_time_ns,
            event_time_ns,
            message: msg_opt,
            process_id,
            sequence_number,
        });
    }

    fn try_emit(&mut self) -> Option<Message> {
        // Get current time as max across all processes
        let current_time = self.current_time_ns();

        if let Some(front) = self.delivery_heap.peek() {
            if self.draining || front.delivery_time_ns <= current_time {
                let pending = self.delivery_heap.pop().unwrap();
                return pending.message;
            }
        }
        None
    }
}

impl MessageStream for MultiProcessGenerator {
    fn next_message(&mut self) -> Option<Message> {
        if self.draining {
            return self.try_emit();
        }

        // Try to emit any ready messages from delivery heap
        if let Some(msg) = self.try_emit() {
            return Some(msg);
        }

        // Select a random process to generate next message
        let process_idx = self.process_selector_rng.gen_range(0..self.processes.len());
        let process = &mut self.processes[process_idx];

        if let Some(msg) = process.next_message() {
            let event_time_ns = match &msg {
                Message::Event { timestamp_ns, .. } | Message::Session { timestamp_ns, .. } => {
                    *timestamp_ns
                }
            };
            let process_id = process.process_id();

            // Apply drop decision
            let msg_opt = self.maybe_drop(msg);

            // Enqueue (even if dropped, to maintain timing)
            self.enqueue(msg_opt, process_id, event_time_ns);
        }

        // Try to emit after enqueuing
        self.try_emit()
    }
}

fn sample_exponential(rng: &mut StdRng, mean: f64) -> f64 {
    // Use gen_range to avoid ln(0) which would produce infinity
    let u = rng.gen_range(f64::MIN_POSITIVE..1.0);
    let sample = -mean * u.ln();
    // Clamp to u64::MAX to prevent overflow when casting
    sample.min(u64::MAX as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::GeneratorConfig;

    fn make_config(seed: u64, interleave_prob: f64, mean_delay_ms: f64) -> GeneratorConfig {
        GeneratorConfig {
            seed: Some(seed),
            event_names: vec!["click".to_string(), "view".to_string()],
            session_types: vec![
                "browsing".to_string(),
                "checkout".to_string(),
                "search".to_string(),
            ],
            interleave_prob,
            mean_delay_ms,
            start_time_ns: 0,
            drop_rate: 0.0,
            num_processes: 1,
            event_rate: 2.0,
            session_gap_mean_ms: 10_000,
            duration_mean_ms: 60_000,
            duration_stddev_ms: 120_000,
        }
    }

    fn make_multi_config(seed: u64, num_processes: u32) -> GeneratorConfig {
        GeneratorConfig {
            seed: Some(seed),
            event_names: vec!["click".to_string(), "view".to_string()],
            session_types: vec![
                "browsing".to_string(),
                "checkout".to_string(),
                "search".to_string(),
            ],
            interleave_prob: 0.0,
            mean_delay_ms: 0.0,
            start_time_ns: 0,
            drop_rate: 0.0,
            num_processes,
            event_rate: 2.0,
            session_gap_mean_ms: 10_000,
            duration_mean_ms: 60_000,
            duration_stddev_ms: 120_000,
        }
    }

    fn timestamp_of(msg: &Message) -> u64 {
        match msg {
            Message::Event { timestamp_ns, .. } | Message::Session { timestamp_ns, .. } => {
                *timestamp_ns
            }
        }
    }

    fn collect_messages(queue: &mut MultiProcessGenerator, count: usize) -> Vec<Message> {
        let mut messages = Vec::new();
        let mut produced = 0;
        while produced < count {
            if let Some(msg) = queue.next_message() {
                messages.push(msg);
                produced += 1;
            }
        }
        queue.start_draining();
        while let Some(msg) = queue.next_message() {
            messages.push(msg);
        }
        messages
    }

    #[test]
    fn test_delivery_queue_deterministic() {
        let config = make_config(42, 0.5, 10.0);
        let mut q1 = MultiProcessGenerator::new(config.clone());
        let mut q2 = MultiProcessGenerator::new(config);

        let msgs1 = collect_messages(&mut q1, 50);
        let msgs2 = collect_messages(&mut q2, 50);

        assert_eq!(msgs1.len(), msgs2.len());
        for (i, (a, b)) in msgs1.iter().zip(msgs2.iter()).enumerate() {
            assert_eq!(a, b, "message {i} differs");
        }
    }

    #[test]
    fn test_no_delay_preserves_timestamp_order() {
        let config = make_config(42, 0.0, 0.0);
        let mut queue = MultiProcessGenerator::new(config);
        let messages = collect_messages(&mut queue, 50);

        let mut prev_ts = 0;
        for (i, msg) in messages.iter().enumerate() {
            let ts = timestamp_of(msg);
            assert!(
                ts >= prev_ts,
                "message {i} timestamp {ts} < previous {prev_ts}"
            );
            prev_ts = ts;
        }
    }

    #[test]
    fn test_delay_causes_reordering() {
        let config = make_config(42, 1.0, 50.0);
        let mut queue = MultiProcessGenerator::new(config);
        let messages = collect_messages(&mut queue, 100);

        let mut found_out_of_order = false;
        let mut prev_ts = 0;
        for msg in &messages {
            let ts = timestamp_of(msg);
            if ts < prev_ts {
                found_out_of_order = true;
                break;
            }
            prev_ts = ts;
        }
        assert!(
            found_out_of_order,
            "expected out-of-order delivery with interleave_prob=1.0 and mean_delay_ms=50"
        );
    }

    #[test]
    fn test_multi_process_deterministic() {
        let config = make_multi_config(42, 3);
        let mut q1 = MultiProcessGenerator::new(config.clone());
        let mut q2 = MultiProcessGenerator::new(config);

        let msgs1 = collect_messages(&mut q1, 100);
        let msgs2 = collect_messages(&mut q2, 100);

        assert_eq!(msgs1.len(), msgs2.len());
        for (i, (a, b)) in msgs1.iter().zip(msgs2.iter()).enumerate() {
            assert_eq!(a, b, "message {i} differs");
        }
    }

    fn process_id_of(msg: &Message) -> u32 {
        match msg {
            Message::Event { process_id, .. } | Message::Session { process_id, .. } => *process_id,
        }
    }

    fn sequence_of(msg: &Message) -> u64 {
        match msg {
            Message::Event {
                sequence_number, ..
            }
            | Message::Session {
                sequence_number, ..
            } => *sequence_number,
        }
    }

    #[test]
    fn test_sequence_numbers_monotonic_per_process() {
        let config = make_multi_config(42, 2);
        let mut queue = MultiProcessGenerator::new(config);
        let messages = collect_messages(&mut queue, 200);

        let mut last_seq: std::collections::HashMap<u32, u64> = std::collections::HashMap::new();

        for msg in &messages {
            let pid = process_id_of(msg);
            let seq = sequence_of(msg);

            if let Some(last) = last_seq.get(&pid) {
                assert_eq!(
                    seq,
                    last + 1,
                    "process {pid}: sequence jumped from {last} to {seq}"
                );
            }
            last_seq.insert(pid, seq);
        }
    }

    #[test]
    fn test_session_start_before_end_timestamps() {
        let config = make_config(42, 0.5, 20.0);
        let mut queue = MultiProcessGenerator::new(config);
        let messages = collect_messages(&mut queue, 200);

        let mut start_times: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();

        for msg in &messages {
            if let Message::Session {
                session_id,
                is_start,
                timestamp_ns,
                ..
            } = msg
            {
                if *is_start {
                    start_times.insert(session_id.clone(), *timestamp_ns);
                } else if let Some(start_ts) = start_times.get(session_id) {
                    assert!(
                        timestamp_ns > start_ts,
                        "session {session_id}: end timestamp {timestamp_ns} <= start timestamp {start_ts}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_drop_rate_creates_gaps() {
        let config = GeneratorConfig {
            seed: Some(42),
            event_names: vec!["click".to_string()],
            session_types: vec!["test".to_string()],
            interleave_prob: 0.0,
            mean_delay_ms: 0.0,
            start_time_ns: 0,
            drop_rate: 0.3,
            num_processes: 1,
            event_rate: 2.0,
            session_gap_mean_ms: 10_000,
            duration_mean_ms: 60_000,
            duration_stddev_ms: 120_000,
        };
        let mut queue = MultiProcessGenerator::new(config);
        let messages = collect_messages(&mut queue, 200);

        // Check for gaps in sequence numbers
        let mut last_seq: Option<u64> = None;
        let mut gap_count = 0;

        for msg in &messages {
            let seq = sequence_of(msg);
            if let Some(last) = last_seq {
                if seq > last + 1 {
                    gap_count += (seq - last - 1) as usize;
                }
            }
            last_seq = Some(seq);
        }

        // With 30% drop rate and 200 messages requested, we should see some gaps
        assert!(gap_count > 0, "expected gaps from dropped messages");
    }
}
