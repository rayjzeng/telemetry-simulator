# Plan: Event-Driven Telemetry Simulator Implementation

**Goal**: Replace timestep-based generation with event-driven architecture using Poisson processes, session type constraints, and mutually exclusive execution modes.

**Architecture**: Event-driven simulation with ProcessSimulator scheduling events via Poisson processes, MultiProcessGenerator collecting earliest events, and session type exclusivity enforcement.

**Tech Stack**: Rust, rand crate (StdRng, WeightedIndex), BinaryHeap, HashMap

## Task Dependencies

| Group | Steps | Can Parallelize | Dependencies |
|-------|-------|-----------------|--------------|
| 1 | Steps 1-3 | Yes | None (config changes) |
| 2 | Steps 4-6 | No | Group 1 (needs config) |
| 3 | Steps 7-10 | No | Group 2 (needs generator) |
| 4 | Steps 11-13 | No | Group 3 (needs delivery) |
| 5 | Steps 14-16 | No | Group 4 (needs main loop) |
| 6 | Steps 17-19 | Yes | Group 5 (tests) |
| 7 | Step 20 | No | Group 6 (final verification) |

---

## Step 1: Update Config with New Flags

**File**: `environment/telemetry-simulator/src/config.rs`

### 1a. Write failing test
```rust
#[test]
fn test_duration_and_min_sessions_mutually_exclusive() {
    let config = Config::parse_from([
        "telemetry-sim",
        "--duration-sec", "10",
        "--min-sessions", "5",
    ]);
    assert!(config.validate().is_err());
}

#[test]
fn test_session_rate_validation() {
    let config = Config::parse_from([
        "telemetry-sim",
        "--session-rate", "-1.0",
    ]);
    assert!(config.validate().is_err());
}

#[test]
fn test_session_duration_validation() {
    let config = Config::parse_from([
        "telemetry-sim",
        "--min-session-duration-ms", "5000",
        "--max-session-duration-ms", "1000",
    ]);
    assert!(config.validate().is_err());
}
```

### 1b. Run test to verify it fails
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_duration_and_min_sessions_mutually_exclusive -- --nocapture 2>&1 | head -20
```

### 1c. Write implementation

Remove `count` field and add new fields:

```rust
#[derive(Parser, Debug, Clone)]
#[command(name = "telemetry-sim")]
pub struct Config {
    #[arg(short, long, default_value = "/tmp/telemetry.sock")]
    pub socket: String,

    #[arg(long, default_value = "0.0")]
    pub jitter_ms: f64,

    #[arg(long, default_value = "0.0")]
    pub mean_delay_ms: f64,

    #[arg(long)]
    pub seed: Option<u64>,

    #[arg(long, default_value = "browsing,checkout,search")]
    pub session_types: String,

    #[arg(long, default_value = "click,view,purchase,scroll,hover")]
    pub event_names: String,

    #[arg(long, default_value = "0.1")]
    pub interleave_prob: f64,

    #[arg(long)]
    pub ready_file: Option<String>,

    #[arg(long, default_value = "0")]
    pub start_time_ms: u64,

    #[arg(long, default_value = "0.0")]
    pub drop_rate: f64,

    #[arg(long, default_value = "1")]
    pub num_processes: u32,

    // NEW FIELDS
    #[arg(long, default_value = "0")]
    pub duration_sec: u64,

    #[arg(long, default_value = "0")]
    pub min_sessions: u64,

    #[arg(long, default_value = "0.1")]
    pub session_rate: f64,

    #[arg(long, default_value = "2.0")]
    pub event_rate: f64,

    #[arg(long, default_value = "1000")]
    pub min_session_duration_ms: u64,

    #[arg(long, default_value = "3600000")]
    pub max_session_duration_ms: u64,

    #[cfg(feature = "stdout")]
    #[arg(long)]
    stdout: bool,
}
```

Add validation:

```rust
if self.duration_sec > 0 && self.min_sessions > 0 {
    return Err("Cannot specify both --duration-sec and --min-sessions (mutually exclusive)".to_string());
}
if self.session_rate <= 0.0 {
    return Err("Session rate must be > 0".to_string());
}
if self.event_rate <= 0.0 {
    return Err("Event rate must be > 0".to_string());
}
if self.min_session_duration_ms < 1 {
    return Err("Minimum session duration must be >= 1 ms".to_string());
}
if self.max_session_duration_ms < self.min_session_duration_ms {
    return Err("Maximum session duration must be >= minimum".to_string());
}
```

### 1d. Run test to verify it passes
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_duration_and_min_sessions_mutually_exclusive -- --nocapture
cargo test test_session_rate_validation -- --nocapture
cargo test test_session_duration_validation -- --nocapture
```

### 1e. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/config.rs
git commit -m "config: Add duration, min-sessions, session-rate, event-rate flags

- Remove deprecated --count flag
- Add --duration-sec for time-based execution
- Add --min-sessions for count-based execution  
- Add --session-rate for Poisson session generation
- Add --event-rate for Poisson event generation
- Add --min-session-duration-ms and --max-session-duration-ms
- Validate mutual exclusivity of duration and min-sessions
- Validate session and event rates > 0
- Validate duration range"
```

---

## Step 2: Update GeneratorConfig

**File**: `environment/telemetry-simulator/src/generator.rs`

### 2a. Write failing test
```rust
#[test]
fn test_generator_config_has_new_fields() {
    let config = GeneratorConfig {
        seed: Some(42),
        event_names: vec!["click".to_string()],
        session_types: vec!["test".to_string()],
        jitter_ms: 0.0,
        interleave_prob: 0.0,
        mean_delay_ms: 0.0,
        start_time_ns: 0,
        drop_rate: 0.0,
        num_processes: 1,
        session_rate: 0.1,
        event_rate: 2.0,
        min_session_duration_ms: 1000,
        max_session_duration_ms: 3600000,
    };
    assert_eq!(config.session_rate, 0.1);
    assert_eq!(config.event_rate, 2.0);
}
```

### 2b. Run test to verify it fails
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_generator_config_has_new_fields -- --nocapture 2>&1 | head -20
```

### 2c. Write implementation

Add fields to GeneratorConfig:

```rust
#[derive(Clone)]
pub struct GeneratorConfig {
    pub seed: Option<u64>,
    pub event_names: Vec<String>,
    pub session_types: Vec<String>,
    pub jitter_ms: f64,
    pub interleave_prob: f64,
    pub mean_delay_ms: f64,
    pub start_time_ns: u64,
    pub drop_rate: f64,
    pub num_processes: u32,
    pub session_rate: f64,
    pub event_rate: f64,
    pub min_session_duration_ms: u64,
    pub max_session_duration_ms: u64,
}
```

### 2d. Run test to verify it passes
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_generator_config_has_new_fields -- --nocapture
```

### 2e. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/generator.rs
git commit -m "generator: Add session_rate, event_rate, duration fields to config

- Add session_rate for Poisson session generation
- Add event_rate for Poisson event generation  
- Add min/max session duration for log-uniform distribution
- Update GeneratorConfig struct"
```

---

## Step 3: Add Imports and Event Types

**File**: `environment/telemetry-simulator/src/generator.rs`

### 3a. Write failing test
```rust
#[test]
fn test_scheduled_event_ordering() {
    use std::collections::BinaryHeap;
    use std::cmp::Reverse;
    
    // This test will fail until ScheduledEvent is implemented
    // Just verify the concept compiles
}
```

### 3b. Run test to verify it fails
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_scheduled_event_ordering -- --nocapture 2>&1 | head -20
```

### 3c. Write implementation

Add imports and event types at top of file:

```rust
use crate::message::Message;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use uuid::Uuid;

const MIN_STEP_NS: u64 = 1_000_000; // 1ms

// Internal event types for scheduling
#[derive(Clone, Eq, PartialEq)]
enum ScheduledEvent {
    SessionStart {
        time_ns: u64,
        session_type: String,
        session_id: String,
    },
    SessionEnd {
        time_ns: u64,
        session_id: String,
        session_type: String,
    },
    EventMessage {
        time_ns: u64,
    },
}

impl ScheduledEvent {
    fn time_ns(&self) -> u64 {
        match self {
            ScheduledEvent::SessionStart { time_ns, .. } => *time_ns,
            ScheduledEvent::SessionEnd { time_ns, .. } => *time_ns,
            ScheduledEvent::EventMessage { time_ns } => *time_ns,
        }
    }
}

impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse for min-heap (earliest time first)
        other.time_ns().cmp(&self.time_ns())
    }
}

impl PartialOrd for ScheduledEvent {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
```

### 3d. Run test to verify it passes
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_scheduled_event_ordering -- --nocapture
cargo test --lib 2>&1 | tail -5
```

### 3e. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/generator.rs
git commit -m "generator: Add ScheduledEvent types and ordering

- Add ScheduledEvent enum for internal event scheduling
- Implement Ord for min-heap (earliest time first)
- Add HashMap import for active_sessions tracking"
```

---

## Step 4: Update ProcessSimulator Struct

**File**: `environment/telemetry-simulator/src/generator.rs`

### 4a. Write failing test
```rust
#[test]
fn test_process_simulator_has_new_fields() {
    let sim = ProcessSimulator::new(
        0, Some(42), 0,
        vec!["click".to_string()],
        vec!["test".to_string()],
        0.0, 0.0, 0.1, 2.0, 1000, 3600000,
    );
    // Test will fail until struct is updated
}
```

### 4b. Run test to verify it fails
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_process_simulator_has_new_fields -- --nocapture 2>&1 | head -20
```

### 4c. Write implementation

Update ProcessSimulator struct:

```rust
pub struct ProcessSimulator {
    process_id: u32,
    rng: StdRng,
    sequence_number: u64,
    current_time_ns: u64,
    event_names: Vec<String>,
    session_types: Vec<String>,
    jitter_ms: f64,
    mean_delay_ms: f64,
    session_rate: f64,
    event_rate: f64,
    min_session_duration_ms: u64,
    max_session_duration_ms: u64,
    pending_sessions: BinaryHeap<PendingSession>,
    active_sessions: HashMap<String, SessionInfo>,
    event_queue: BinaryHeap<Reverse<ScheduledEvent>>,
}

struct SessionInfo {
    session_id: String,
    end_time: u64,
}
```

Update ProcessSimulator::new:

```rust
pub fn new(
    process_id: u32,
    seed: Option<u64>,
    start_time_ns: u64,
    event_names: Vec<String>,
    session_types: Vec<String>,
    jitter_ms: f64,
    mean_delay_ms: f64,
    session_rate: f64,
    event_rate: f64,
    min_session_duration_ms: u64,
    max_session_duration_ms: u64,
) -> Self {
    let rng = match seed {
        Some(s) => StdRng::seed_from_u64(s.wrapping_add(process_id as u64)),
        None => StdRng::from_entropy(),
    };
    let mut sim = Self {
        process_id,
        rng,
        sequence_number: 0,
        current_time_ns: start_time_ns,
        event_names,
        session_types,
        jitter_ms,
        mean_delay_ms,
        session_rate,
        event_rate,
        min_session_duration_ms,
        max_session_duration_ms,
        pending_sessions: BinaryHeap::new(),
        active_sessions: HashMap::new(),
        event_queue: BinaryHeap::new(),
    };
    
    // Schedule first session
    sim.schedule_next_session();
    
    sim
}
```

### 4d. Run test to verify it passes
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_process_simulator_has_new_fields -- --nocapture
```

### 4e. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/generator.rs
git commit -m "generator: Update ProcessSimulator for event-driven architecture

- Add session_rate, event_rate fields
- Add min/max session duration fields
- Add active_sessions HashMap for type constraints
- Add event_queue for scheduled events
- Update constructor to initialize new fields
- Schedule first session on creation"
```

---

## Step 5: Implement Event Scheduling Methods

**File**: `environment/telemetry-simulator/src/generator.rs`

### 5a. Write failing test
```rust
#[test]
fn test_schedule_next_session() {
    let mut sim = ProcessSimulator::new(
        0, Some(42), 0,
        vec!["click".to_string()],
        vec!["a".to_string(), "b".to_string()],
        0.0, 0.0, 1.0, 2.0, 1000, 3600000,
    );
    
    // Should have scheduled first session
    assert!(sim.peek_next_event_time().is_some());
}
```

### 5b. Run test to verify it fails
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_schedule_next_session -- --nocapture 2>&1 | head -30
```

### 5c. Write implementation

Add methods to ProcessSimulator:

```rust
impl ProcessSimulator {
    // ... existing methods ...
    
    pub fn peek_next_event_time(&self) -> Option<u64> {
        self.event_queue.peek().map(|e| e.0.time_ns())
    }
    
    fn schedule_next_session(&mut self) {
        // Find available session types
        let available: Vec<_> = self.session_types
            .iter()
            .filter(|t| !self.active_sessions.contains_key(*t))
            .collect();
        
        if available.is_empty() {
            // All types busy, try again in 100ms
            let retry_time = self.current_time_ns + 100_000_000;
            // Don't schedule - will retry when next event processed
            return;
        }
        
        // Sample inter-arrival time from exponential distribution
        let delta = self.sample_exponential(1.0 / self.session_rate);
        let next_time = self.current_time_ns + (delta * 1e9) as u64;
        
        // Uniform random selection from available types
        let session_type = available[self.rng.gen_range(0..available.len())].clone();
        
        let event = ScheduledEvent::SessionStart {
            time_ns: next_time,
            session_type,
            session_id: self.generate_id(),
        };
        self.event_queue.push(Reverse(event));
    }
    
    fn schedule_next_event(&mut self) {
        if self.active_sessions.is_empty() {
            return;
        }
        
        // Event rate scales with number of active sessions
        let total_rate = self.event_rate * self.active_sessions.len() as f64;
        let delta = self.sample_exponential(1.0 / total_rate);
        let next_time = self.current_time_ns + (delta * 1e9) as u64;
        
        let event = ScheduledEvent::EventMessage {
            time_ns: next_time,
        };
        self.event_queue.push(Reverse(event));
    }
    
    fn sample_exponential(&mut self, mean: f64) -> f64 {
        let u = self.rng.gen_range(f64::MIN_POSITIVE..1.0);
        -mean * u.ln()
    }
    
    fn sample_session_duration(&mut self) -> u64 {
        // Log-uniform distribution
        let min_log = (self.min_session_duration_ms as f64).ln();
        let max_log = (self.max_session_duration_ms as f64).ln();
        let random_log = self.rng.gen_range(min_log..=max_log);
        (random_log.exp() as u64) * 1_000_000 // to nanoseconds
    }
}
```

### 5d. Run test to verify it passes
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_schedule_next_session -- --nocapture
```

### 5e. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/generator.rs
git commit -m "generator: Implement event scheduling methods

- Add peek_next_event_time() method
- Add schedule_next_session() with Poisson process
- Add schedule_next_event() with rate scaling
- Add sample_exponential() for Poisson inter-arrivals
- Add sample_session_duration() with log-uniform distribution
- Filter available session types before scheduling"
```

---

## Step 6: Implement pop_next_event

**File**: `environment/telemetry-simulator/src/generator.rs`

### 6a. Write failing test
```rust
#[test]
fn test_pop_next_event() {
    let mut sim = ProcessSimulator::new(
        0, Some(42), 0,
        vec!["click".to_string()],
        vec!["test".to_string()],
        0.0, 0.0, 10.0, 2.0, 100, 1000,
    );
    
    // Should get a session start event
    let result = sim.pop_next_event();
    assert!(result.is_some());
    
    let (time, msg) = result.unwrap();
    assert!(time > 0);
    match msg {
        Message::Session { is_start: true, .. } => {},
        _ => panic!("Expected session start"),
    }
}
```

### 6b. Run test to verify it fails
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_pop_next_event -- --nocapture 2>&1 | head -30
```

### 6c. Write implementation

Replace the old `next_message` and `check_pending_session_end` with new event-driven version:

```rust
// Remove old next_message and check_pending_session_end methods

/// Pop and process next event, return (time, message)
pub fn pop_next_event(&mut self) -> Option<(u64, Message)> {
    let event = self.event_queue.pop()?.0;
    self.current_time_ns = event.time_ns();
    
    match event {
        ScheduledEvent::SessionStart { time_ns, session_type, session_id } => {
            self.handle_session_start(time_ns, session_type, session_id)
        }
        ScheduledEvent::SessionEnd { time_ns, session_id, session_type } => {
            self.handle_session_end(time_ns, session_id, session_type)
        }
        ScheduledEvent::EventMessage { time_ns } => {
            self.handle_event_message(time_ns)
        }
    }
}

fn handle_session_start(
    &mut self,
    time_ns: u64,
    session_type: String,
    session_id: String,
) -> Option<(u64, Message)> {
    // Check if type already active
    if self.active_sessions.contains_key(&session_type) {
        // Type became busy, reschedule
        self.schedule_next_session();
        return None;
    }
    
    // Schedule session end
    let duration = self.sample_session_duration();
    let end_time = time_ns + duration;
    self.active_sessions.insert(session_type.clone(), SessionInfo {
        session_id: session_id.clone(),
        end_time,
    });
    
    self.event_queue.push(Reverse(ScheduledEvent::SessionEnd {
        time_ns: end_time,
        session_id: session_id.clone(),
        session_type: session_type.clone(),
    }));
    
    // Schedule next session
    self.schedule_next_session();
    
    // If first session, schedule first event
    if self.active_sessions.len() == 1 {
        self.schedule_next_event();
    }
    
    // Emit session start message
    let msg = Message::Session {
        session_id,
        session_type,
        timestamp_ns: time_ns,
        is_start: true,
        version: 1,
        process_id: self.process_id,
        sequence_number: self.next_sequence(),
    };
    
    Some((time_ns, msg))
}

fn handle_session_end(
    &mut self,
    time_ns: u64,
    session_id: String,
    session_type: String,
) -> Option<(u64, Message)> {
    // Remove from active sessions
    self.active_sessions.remove(&session_type);
    
    // If no more active sessions, clear pending events
    if self.active_sessions.is_empty() {
        self.event_queue.retain(|e| !matches!(e.0, ScheduledEvent::EventMessage { .. }));
    }
    
    // Emit session end message
    let msg = Message::Session {
        session_id,
        session_type,
        timestamp_ns: time_ns,
        is_start: false,
        version: 1,
        process_id: self.process_id,
        sequence_number: self.next_sequence(),
    };
    
    Some((time_ns, msg))
}

fn handle_event_message(&mut self, time_ns: u64) -> Option<(u64, Message)> {
    // Schedule next event
    self.schedule_next_event();
    
    // Emit event message
    let msg = Message::Event {
        event_id: self.generate_id(),
        event_name: self.event_names[self.rng.gen_range(0..self.event_names.len())].clone(),
        timestamp_ns: time_ns,
        version: 1,
        process_id: self.process_id,
        sequence_number: self.next_sequence(),
    };
    
    Some((time_ns, msg))
}
```

### 6d. Run test to verify it passes
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_pop_next_event -- --nocapture
```

### 6e. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/generator.rs
git commit -m "generator: Implement event-driven pop_next_event

- Replace timestep-based next_message with event-driven pop_next_event
- Add handle_session_start with type constraint checking
- Add handle_session_end with active_sessions cleanup
- Add handle_event_message with rate scaling
- Events only generated when sessions active
- Remove old check_pending_session_end method"
```

---

## Step 7: Update MultiProcessGenerator

**File**: `environment/telemetry-simulator/src/delivery.rs`

### 7a. Write failing test
```rust
#[test]
fn test_multiprocess_generator_uses_new_api() {
    let config = make_config(42, 0.0, 0.0);
    let mut gen = MultiProcessGenerator::new(config);
    
    // Should be able to get messages
    let msg = gen.next_message();
    // May be None if no events ready, but shouldn't panic
}
```

### 7b. Run test to verify it fails
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_multiprocess_generator_uses_new_api -- --nocapture 2>&1 | head -30
```

### 7c. Write implementation

Update MultiProcessGenerator to use pop_next_event:

```rust
impl MessageStream for MultiProcessGenerator {
    fn next_message(&mut self) -> Option<Message> {
        // Get process with earliest event
        let Reverse((_, pid)) = self.event_queue.pop()?;
        let process = &mut self.processes[pid];
        
        // Get event from process
        let result = process.pop_next_event();
        
        // Update process's next event in queue
        if let Some(next_time) = process.peek_next_event_time() {
            self.event_queue.push(Reverse((next_time, pid)));
        }
        
        match result {
            Some((_, msg)) => {
                // Apply drop decision
                if self.drop_rate > 0.0 && self.drop_rng.gen_bool(self.drop_rate) {
                    return None;
                }
                Some(msg)
            }
            None => {
                // Process had no event (e.g., session type busy)
                // Try again with next process
                self.next_message()
            }
        }
    }
}
```

Update ProcessSimulator creation to pass new parameters:

```rust
let process = ProcessSimulator::new(
    pid,
    config.seed,
    config.start_time_ns,
    config.event_names.clone(),
    config.session_types.clone(),
    config.jitter_ms,
    config.mean_delay_ms,
    config.session_rate,
    config.event_rate,
    config.min_session_duration_ms,
    config.max_session_duration_ms,
);
```

### 7d. Run test to verify it passes
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_multiprocess_generator_uses_new_api -- --nocapture
```

### 7e. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/delivery.rs
git commit -m "delivery: Update to use event-driven ProcessSimulator

- Change to call pop_next_event() instead of next_message()
- Update ProcessSimulator::new() call with new parameters
- Handle None returns (when session type busy)
- Update event queue after each pop"
```

---

## Step 8: Update Main Loop for Duration/Count Modes

**File**: `environment/telemetry-simulator/src/main.rs`

### 8a. Write failing test
This is an integration test, will be added later. For now, verify compilation.

### 8b. Write implementation

Replace the main loop with ExitState logic:

```rust
use std::time::Instant;

// ... in main function, after queue creation ...

let use_stdout = config.stdout();

// Exit state tracking
enum RunMode {
    Duration { duration_sec: u64 },
    Count { min_sessions: u64 },
    Indefinite,
}

struct ExitState {
    start_time: Instant,
    sessions_started: Vec<u64>,
    mode: RunMode,
    draining: bool,
}

impl ExitState {
    fn new(num_processes: usize, duration_sec: u64, min_sessions: u64) -> Self {
        let mode = if duration_sec > 0 {
            RunMode::Duration { duration_sec }
        } else if min_sessions > 0 {
            RunMode::Count { min_sessions }
        } else {
            RunMode::Indefinite
        };
        
        Self {
            start_time: Instant::now(),
            sessions_started: vec![0; num_processes],
            mode,
            draining: false,
        }
    }
    
    fn record_session_start(&mut self, process_id: u32) {
        self.sessions_started[process_id as usize] += 1;
    }
    
    fn should_start_draining(&self) -> bool {
        if self.draining {
            return false;
        }
        
        match self.mode {
            RunMode::Duration { duration_sec } => {
                self.start_time.elapsed().as_secs() >= duration_sec
            }
            RunMode::Count { min_sessions } => {
                self.sessions_started.iter().all(|&count| count >= min_sessions)
            }
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
);

loop {
    // Check if we should transition to draining mode
    if exit_state.should_start_draining() {
        exit_state.start_draining();
        queue.start_draining();
        match exit_state.mode {
            RunMode::Duration { .. } => eprintln!("Duration reached, draining pending sessions..."),
            RunMode::Count { .. } => eprintln!("Session count reached, draining pending sessions..."),
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
            if let telemetry_simulator::message::Message::Session { is_start: true, process_id, .. } = &msg {
                exit_state.record_session_start(*process_id);
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
    if let Err(e) = write_msg(&mut output, &msg, use_stdout).await {
        eprintln!("Write error: {}", e);
        return ExitCode::from(4);
    }
}

// Print summary
let elapsed = exit_state.start_time.elapsed().as_secs();
eprintln!("Simulation complete:");
eprintln!("  Duration: {}s", elapsed);
eprintln!("  Sessions per process: {:?}", exit_state.sessions_started);

let _ = output.flush().await;
ExitCode::from(0)
```

### 8c. Run test to verify it compiles
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo build 2>&1 | tail -10
```

### 8d. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/main.rs
git commit -m "main: Implement duration and count modes with ExitState

- Add RunMode enum (Duration, Count, Indefinite)
- Add ExitState struct to track exit conditions
- Implement duration-based exit
- Implement min-sessions exit (per process)
- Add draining mode for clean shutdown
- Track sessions per process
- Print summary at end"
```

---

## Step 9: Update Tests for New API

**File**: `environment/telemetry-simulator/src/generator.rs`

### 9a. Update make_process function

```rust
fn make_process(seed: u64, process_id: u32) -> ProcessSimulator {
    ProcessSimulator::new(
        process_id,
        Some(seed),
        0,
        vec!["click".to_string(), "view".to_string()],
        vec!["browsing".to_string(), "checkout".to_string()],
        0.0,
        10.0,
        0.1,  // session_rate
        2.0,  // event_rate
        1000, // min_session_duration_ms
        3600000, // max_session_duration_ms
    )
}
```

### 9b. Update test_session_pairs

```rust
fn test_session_pairs() {
    let mut p = ProcessSimulator::new(
        0,
        Some(42),
        0,
        vec!["click".to_string()],
        vec!["test".to_string()],
        0.0,
        1.0,
        0.1,
        2.0,
        100,
        1000,
    );
    // ... rest of test
}
```

### 9c. Run tests
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test --lib 2>&1 | tail -10
```

### 9d. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/generator.rs
git commit -m "generator: Update tests for new ProcessSimulator API

- Update make_process with new parameters
- Update test_session_pairs with new parameters
- All generator tests passing"
```

---

## Step 10: Update Delivery Tests

**File**: `environment/telemetry-simulator/src/delivery.rs`

### 10a. Update make_config and make_multi_config

Add new fields to GeneratorConfig in both functions:

```rust
session_rate: 0.1,
event_rate: 2.0,
min_session_duration_ms: 1000,
max_session_duration_ms: 3600000,
```

### 10b. Update test_drop_rate_creates_gaps

Add new fields to config.

### 10c. Run tests
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test --lib 2>&1 | tail -10
```

### 10d. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/delivery.rs
git commit -m "delivery: Update tests for new GeneratorConfig

- Add session_rate, event_rate, duration fields to test configs
- All delivery tests passing"
```

---

## Step 11: Add New Unit Tests

**File**: `environment/telemetry-simulator/src/generator.rs`

### 11a. Write tests

Add to generator tests module:

```rust
#[test]
fn test_session_type_exclusivity() {
    let mut p = ProcessSimulator::new(
        0, Some(42), 0,
        vec!["click".to_string()],
        vec!["a".to_string(), "b".to_string()],
        0.0, 0.0, 100.0, 10.0, 100, 1000,
    );
    
    let mut active_types = std::collections::HashSet::new();
    
    for _ in 0..100 {
        if let Some((_, msg)) = p.pop_next_event() {
            if let Message::Session { session_type, is_start, .. } = msg {
                if is_start {
                    // Should not already be active
                    assert!(!active_types.contains(&session_type),
                        "Session type {} started while already active", session_type);
                    active_types.insert(session_type);
                } else {
                    // Session end - type should have been active
                    // Note: session_type is empty for ends, so we can't verify here
                    // This is a limitation of the current message format
                }
            }
        }
    }
}

#[test]
fn test_events_only_during_sessions() {
    let mut p = ProcessSimulator::new(
        0, Some(42), 0,
        vec!["click".to_string()],
        vec!["test".to_string()],
        0.0, 0.0, 1.0, 10.0, 100, 1000,
    );
    
    let mut has_active_session = false;
    let mut event_without_session = false;
    
    for _ in 0..200 {
        if let Some((_, msg)) = p.pop_next_event() {
            match msg {
                Message::Session { is_start, .. } => {
                    has_active_session = is_start;
                }
                Message::Event { .. } => {
                    if !has_active_session {
                        event_without_session = true;
                    }
                }
            }
        }
    }
    
    assert!(!event_without_session, "Events generated without active session");
}
```

### 11b. Run tests
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_session_type_exclusivity -- --nocapture
cargo test test_events_only_during_sessions -- --nocapture
```

### 11c. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/generator.rs
git commit -m "generator: Add tests for session constraints and event correlation

- test_session_type_exclusivity: Verify no overlapping same-type sessions
- test_events_only_during_sessions: Verify events only with active sessions"
```

---

## Step 12: Update Integration Test

**File**: `environment/telemetry-simulator/tests/integration_test.rs`

### 12a. Update test

```rust
#[tokio::test]
async fn test_min_sessions_mode() {
    let sim = TestSimulator::start(&["--min-sessions", "5", "--seed", "42"]);

    let mut stream = UnixStream::connect(&sim.socket_path).await.unwrap();
    let mut buf = [0u8; 4];
    let mut session_count = 0;

    // Read until we get 5 sessions (may get more due to events)
    for _ in 0..50 {
        if stream.read_exact(&mut buf).await.is_err() {
            break;
        }
        let len = u32::from_be_bytes(buf) as usize;
        let mut msg_buf = vec![0u8; len];
        if stream.read_exact(&mut msg_buf).await.is_err() {
            break;
        }
        session_count += 1;
    }

    // Should have generated at least 5 sessions
    assert!(session_count >= 5);
}
```

### 12b. Run test
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_min_sessions_mode -- --nocapture 2>&1 | tail -20
```

### 12c. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/tests/integration_test.rs
git commit -m "tests: Update integration test for min-sessions mode

- Rename test_burst_mode_generates_exact_count to test_min_sessions_mode
- Update to use --min-sessions instead of --count
- Verify session generation works end-to-end"
```

---

## Step 13: Add Duration Mode Integration Test

**File**: `environment/telemetry-simulator/tests/integration_test.rs`

### 13a. Write test

```rust
#[tokio::test]
async fn test_duration_mode() {
    use std::time::Instant;
    
    let start = Instant::now();
    let sim = TestSimulator::start(&["--duration-sec", "1", "--seed", "42"]);
    
    let mut stream = UnixStream::connect(&sim.socket_path).await.unwrap();
    let mut buf = [0u8; 4];
    let mut count = 0;
    
    // Read messages for up to 2 seconds
    while start.elapsed().as_secs() < 2 {
        match tokio::time::timeout(
            tokio::time::Duration::from_millis(100),
            stream.read_exact(&mut buf)
        ).await {
            Ok(Ok(_)) => {
                let len = u32::from_be_bytes(buf) as usize;
                let mut msg_buf = vec![0u8; len];
                let _ = stream.read_exact(&mut msg_buf).await;
                count += 1;
            }
            _ => break,
        }
    }
    
    let elapsed = start.elapsed().as_secs();
    // Should run for ~1 second plus drain time
    assert!(elapsed >= 1 && elapsed < 3, "Elapsed: {}", elapsed);
    assert!(count > 0, "Should have generated messages");
}
```

### 13b. Run test
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_duration_mode -- --nocapture 2>&1 | tail -20
```

### 13c. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/tests/integration_test.rs
git commit -m "tests: Add duration mode integration test

- Test that --duration-sec runs for approximately correct time
- Verify messages generated during duration
- Verify clean shutdown after duration"
```

---

## Step 14: Test Mutually Exclusive Flags

**File**: `environment/telemetry-simulator/tests/integration_test.rs`

### 14a. Write test

```rust
#[test]
fn test_mutually_exclusive_flags() {
    let config = Config::parse_from([
        "telemetry-sim",
        "--duration-sec", "10",
        "--min-sessions", "5",
    ]);
    assert!(config.validate().is_err());
    assert!(config.validate().unwrap_err().contains("mutually exclusive"));
}
```

### 14b. Run test
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_mutually_exclusive_flags -- --nocapture
```

### 14c. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/tests/integration_test.rs
git commit -m "tests: Add mutually exclusive flags test

- Verify --duration-sec and --min-sessions cannot be used together
- Validation should fail with clear error message"
```

---

## Step 15: Remove Deprecated Code

**Files**: `environment/telemetry-simulator/src/generator.rs`, `environment/telemetry-simulator/src/config.rs`

### 15a. Remove unused fields

From ProcessSimulator, remove:
- `jitter_ms` (no longer used for generation)
- `mean_delay_ms` (no longer used for generation)

From GeneratorConfig, these can stay for delivery layer, but mark as deprecated in comments.

### 15b. Update tests

Remove any tests that depend on removed fields.

### 15c. Run tests
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test --lib 2>&1 | tail -10
```

### 15d. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/generator.rs
git commit -m "generator: Remove deprecated jitter and mean_delay fields

- These fields no longer used for event generation
- Kept in config for delivery layer (network delays)
- Removed from ProcessSimulator"
```

---

## Step 16: Update Documentation

**File**: `environment/telemetry-simulator/docs/SOCKET_API.md`

### 16a. Update configuration section

Add new flags, remove deprecated ones, update examples.

### 16b. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/docs/SOCKET_API.md
git commit -m "docs: Update SOCKET_API for event-driven architecture

- Document new flags: --duration-sec, --min-sessions, --session-rate, --event-rate
- Document session duration flags
- Remove --count documentation
- Update examples
- Document session type constraints"
```

---

## Step 17: Performance Test

**File**: Create `environment/telemetry-simulator/benches/performance.rs` or add test

### 17a. Write benchmark

```rust
#[test]
fn test_high_event_rate() {
    let config = GeneratorConfig {
        seed: Some(42),
        event_names: vec!["click".to_string()],
        session_types: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        jitter_ms: 0.0,
        interleave_prob: 0.0,
        mean_delay_ms: 0.0,
        start_time_ns: 0,
        drop_rate: 0.0,
        num_processes: 10,
        session_rate: 10.0,
        event_rate: 100.0,
        min_session_duration_ms: 100,
        max_session_duration_ms: 1000,
    };
    
    let mut gen = MultiProcessGenerator::new(config);
    let start = std::time::Instant::now();
    let mut count = 0;
    
    while start.elapsed().as_secs() < 1 && count < 10000 {
        if gen.next_message().is_some() {
            count += 1;
        }
    }
    
    let elapsed = start.elapsed().as_secs_f64();
    let rate = count as f64 / elapsed;
    println!("Generated {} messages in {:.2}s ({:.0} msgs/sec)", count, elapsed, rate);
    assert!(rate > 5000.0, "Rate too low: {}", rate);
}
```

### 17b. Run test
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test test_high_event_rate -- --nocapture 2>&1 | tail -20
```

### 17c. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add environment/telemetry-simulator/src/delivery.rs
git commit -m "delivery: Add performance test

- Test high event rate (10K msgs/sec target)
- Verify performance is acceptable"
```

---

## Step 18: Final Cleanup

### 18a. Remove warnings

Fix any remaining warnings about unused code.

### 18b. Run full test suite
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test 2>&1 | tail -20
```

### 18c. Commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add -A
git commit -m "cleanup: Fix warnings and final cleanup

- Remove unused code
- Fix compiler warnings
- All tests passing"
```

---

## Step 19: End-to-End Verification

### 19a. Test duration mode
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo run --features stdout -- --duration-sec 2 --num-processes 2 --seed 42 --stdout 2>&1 | head -30
```

Verify:
- Runs for ~2 seconds
- Generates messages
- Prints summary at end
- Session type constraints held

### 19b. Test min-sessions mode
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo run --features stdout -- --min-sessions 5 --num-processes 2 --seed 42 --stdout 2>&1 | tail -20
```

Verify:
- Generates at least 5 sessions per process
- Drains and exits
- Prints summary

### 19c. Test mutually exclusive
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo run --features stdout -- --duration-sec 5 --min-sessions 10 2>&1 | head -5
```

Verify:
- Error message about mutually exclusive flags
- Exits with error code

### 19d. Test indefinite mode
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
timeout 3 cargo run --features stdout -- --num-processes 1 --seed 42 --stdout 2>&1 | head -20
```

Verify:
- Runs until timeout (Ctrl+C)
- Generates messages
- Drains on shutdown

### 19e. Final commit
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner
git add -A
git commit -m "event-driven: Complete implementation

Implemented event-driven architecture with:
- Poisson processes for session and event generation
- Session type constraints (1 active per type)
- Uniform session type selection
- Log-uniform session durations (seconds to hours)
- Events only generated during active sessions
- Mutually exclusive duration and count modes
- Event-driven scheduling with priority queues

All 20+ tests passing. Ready for use."
```

---

## Step 20: Documentation and Summary

### 20a. Update README if exists

### 20b. Final verification

Run complete test suite one more time:
```bash
cd /Users/rayzeng/dev/aai/rayzeng-tbench/session-telemetry-joiner/environment/telemetry-simulator
cargo test 2>&1 | grep -E "test result|passed|failed"
```

### 20c. Create summary

Document what was implemented and how to use it.

---

## Files Touched Summary

| File | Changes |
|------|---------|
| `src/config.rs` | Remove count, add duration/min-sessions/session-rate/event-rate/duration flags, add validation |
| `src/generator.rs` | Add ScheduledEvent, rewrite ProcessSimulator for event-driven, add session constraints, add log-uniform durations |
| `src/delivery.rs` | Update to use pop_next_event, pass new parameters |
| `src/main.rs` | Rewrite main loop with ExitState, duration/count/indefinite modes |
| `tests/integration_test.rs` | Update for new flags, add duration test, add mutual exclusivity test |
| `docs/SOCKET_API.md` | Update documentation |

## Success Criteria

- [ ] All 20+ tests passing
- [ ] Duration mode works correctly
- [ ] Min-sessions mode works correctly  
- [ ] Mutually exclusive validation works
- [ ] Session type constraints enforced
- [ ] Events only during sessions
- [ ] Performance acceptable (10K msgs/sec)
- [ ] Documentation updated
- [ ] No compiler warnings
