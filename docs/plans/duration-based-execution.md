# Duration-Based Execution Plan

## Overview

Replace the `--count` flag with two mutually exclusive modes:
1. **Duration mode**: Run for a specified amount of time with realistic session behavior
2. **Count mode**: Generate a specific number of sessions per process (for testing)

Additionally, enforce session type constraints: for a given session_type, only one active session can exist at a time within a process. Different session types can overlap. Session durations range from seconds to hours to simulate realistic web application usage.

## Goals

1. Replace `--count` with two mutually exclusive modes:
   - `--duration-sec`: Run for specified time with realistic sessions
   - `--min-sessions`: Generate exact number of sessions per process (testing mode)
2. Support indefinite execution when neither flag specified
3. Drain pending sessions before exit
4. Enforce single active session per session_type per process
5. Support realistic session durations (seconds to hours)
6. Prioritize realistic behavior over strict guarantees

## Configuration Changes

### Remove

| Flag | Description |
|------|-------------|
| `--count` | Exact message count (deprecated) |

### Add

| Flag | Type | Default | Description |
|------|------|---------|-------------|
| `--duration-sec` | u64 | 0 | Run for N seconds (0 = run indefinitely unless min-sessions specified) |
| `--min-sessions` | u64 | 0 | Generate exactly N sessions per process then exit (0 = disabled) |
| `--session-prob` | f64 | 0.1 | Probability of starting a session per iteration |
| `--min-session-duration-ms` | u64 | 1000 | Minimum session duration in milliseconds |
| `--max-session-duration-ms` | u64 | 3600000 | Maximum session duration in milliseconds (1 hour default) |

### Validation Rules

1. `--duration-sec` >= 0
2. `--min-sessions` >= 0
3. `--session-prob` between 0.0 and 1.0
4. `--min-session-duration-ms` >= 1
5. `--max-session-duration-ms` >= `--min-session-duration-ms`
6. **Mutually exclusive**: Cannot specify both `--duration-sec` > 0 and `--min-sessions` > 0
7. If `--duration-sec` == 0 and `--min-sessions` == 0: run indefinitely until Ctrl+C

## Session Type Constraints

### Requirements

1. **Single active session per type**: For a given `session_type` within a process, only one session can be active at a time
2. **Cross-type overlap allowed**: Different session types can have overlapping active sessions
3. **Per-process isolation**: Each process tracks its own active sessions independently

### Example

Process 0 with session types ["browsing", "checkout", "search"]:

```
Time 0ms:  Start browsing session A1 ✓ (no active browsing)
Time 5ms:  Start checkout session B1 ✓ (no active checkout, browsing can overlap)
Time 10ms: Try to start browsing session A2 ✗ (browsing A1 still active)
Time 15ms: Start search session C1 ✓ (no active search)
Time 20ms: End browsing session A1
Time 25ms: Start browsing session A2 ✓ (browsing A1 ended, now allowed)
```

### Implementation

**ProcessSimulator** will track active sessions by type:

```rust
pub struct ProcessSimulator {
    // ... existing fields
    active_sessions: HashMap<String, String>, // session_type -> session_id
}

impl ProcessSimulator {
    fn can_start_session(&self, session_type: &str) -> bool {
        !self.active_sessions.contains_key(session_type)
    }
    
    fn start_session(&mut self, session_type: String, session_id: String) {
        self.active_sessions.insert(session_type, session_id);
    }
    
    fn end_session(&mut self, session_type: &str) {
        self.active_sessions.remove(session_type);
    }
}
```

**Session generation logic:**

```rust
pub fn next_message(&mut self) -> Option<Message> {
    // ... check pending sessions ...
    
    // Try to start a new session
    if self.rng.gen_bool(self.session_prob) {
        // Find available session types (no active session)
        let available_types: Vec<_> = self.session_types
            .iter()
            .filter(|t| self.can_start_session(t))
            .collect();
        
        if !available_types.is_empty() {
            // Select random available type
            let session_type = available_types[self.rng.gen_range(0..available_types.len())].clone();
            let session_id = self.generate_id();
            
            // Mark as active
            self.start_session(session_type.clone(), session_id.clone());
            
            // Schedule end and emit start...
        }
        // If no available types, fall through to event generation
    }
    
    // Generate event...
}
```

**Session end handling:**

When a session end is emitted from pending_sessions, also remove it from active_sessions:

```rust
fn check_pending_session_end(&mut self) -> Option<Message> {
    if let Some(pending) = self.pending_sessions.peek() {
        if pending.end_time_ns <= self.synthetic_time_ns {
            let pending = self.pending_sessions.pop().unwrap();
            
            // Remove from active sessions
            self.end_session(&pending.session_type);
            
            // Emit session end message...
        }
    }
    None
}
```

### Impact on Session Generation Rate

With session type constraints, the actual session generation rate may be lower than `session_prob` suggests, especially when:
- Few session types configured
- Sessions have long durations
- All session types are currently active

**Mitigation:**
- The auto-tuning formula accounts for this by using observed rates
- If all types are busy, session attempts are skipped (no error)
- Users can increase `session_prob` or add more session types

## Session Duration Configuration

### Realistic Web Application Sessions

Session durations should reflect real-world web application usage:
- **Short sessions**: 1-10 seconds (quick page views)
- **Medium sessions**: 10 seconds - 5 minutes (browsing, reading)
- **Long sessions**: 5 minutes - 1 hour (checkout flows, extended browsing)
- **Very long sessions**: 1+ hours (dashboard monitoring, background tabs)

### Configuration

Users can configure session duration range:
- `--min-session-duration-ms`: Minimum session duration (default: 1000ms = 1s)
- `--max-session-duration-ms`: Maximum session duration (default: 3600000ms = 1 hour)

### Duration Selection

Session durations are randomly selected from a **log-uniform distribution** to better reflect real-world patterns (many short sessions, fewer long sessions):

```rust
fn random_session_duration(
    rng: &mut StdRng,
    min_ms: u64,
    max_ms: u64,
) -> u64 {
    // Log-uniform: more short sessions, fewer long sessions
    let min_log = (min_ms as f64).ln();
    let max_log = (max_ms as f64).ln();
    let random_log = rng.gen_range(min_log..=max_log);
    random_log.exp() as u64
}
```

This produces a more realistic distribution than uniform random.

## Execution Logic

### Two Operating Modes

The simulator operates in one of two mutually exclusive modes:

#### Mode 1: Duration Mode (`--duration-sec` specified)

Run for a fixed amount of time with realistic session behavior.

**Exit condition:** `elapsed >= duration_sec`

#### Mode 2: Count Mode (`--min-sessions` specified)

Generate exact number of sessions per process, then drain and exit.

**Exit condition:** `all processes have sessions_started >= min_sessions`

#### Mode 3: Indefinite Mode (neither specified)

Run until Ctrl+C, then drain and exit.

### State Machine

```
┌─────────────┐
│   Running   │◄────┐
└──────┬──────┘     │
       │            │
       ▼            │
┌─────────────┐     │
│ Check Exit  │     │
│ Conditions  │     │
└──────┬──────┘     │
       │            │
       ├─ No ───────┘
       │
       ▼ Yes
┌─────────────┐
│   Draining  │
└──────┬──────┘
       │
       ▼
     Exit
```

### Exit Conditions

**Duration Mode:**
1. **Duration satisfied**: `elapsed >= duration_sec`
2. **Drain complete**: All pending session ends emitted

**Count Mode:**
1. **Session count satisfied**: `all processes have sessions_started >= min_sessions`
2. **Drain complete**: All pending session ends emitted

**Indefinite Mode:**
1. **User interrupt**: Ctrl+C received
2. **Drain complete**: All pending session ends emitted

### Exit State Tracking

```rust
enum RunMode {
    Duration { duration_sec: u64 },
    Count { min_sessions: u64 },
    Indefinite,
}

struct ExitState {
    start_time: Instant,
    sessions_started: Vec<u64>, // per process counter
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
```

### Main Loop Pseudocode

```rust
let mut exit_state = ExitState::new(
    config.num_processes as usize,
    config.duration_sec,
    config.min_sessions,
);

let gen_config = GeneratorConfig {
    // ... other fields
    session_prob: config.session_prob,
    min_session_duration_ms: config.min_session_duration_ms,
    max_session_duration_ms: config.max_session_duration_ms,
};

let mut queue = MultiProcessGenerator::new(gen_config);

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
            if let Message::Session { is_start: true, process_id, .. } = &msg {
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
            tokio::time::sleep(Duration::from_millis(1)).await;
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
```

## Module Changes

### `src/config.rs`

**Remove:**
- `count` field

**Add:**
```rust
#[arg(long, default_value = "0")]
pub duration_sec: u64,

#[arg(long, default_value = "0")]
pub min_sessions: u64,

#[arg(long, default_value = "0.1")]
pub session_prob: f64,
```

**Validation:**
```rust
if self.session_prob < 0.0 || self.session_prob > 1.0 {
    return Err("Session probability must be between 0.0 and 1.0".to_string());
}

if self.duration_sec > 0 && self.min_sessions > 0 {
    return Err("Cannot specify both --duration-sec and --min-sessions (mutually exclusive)".to_string());
}

if self.min_session_duration_ms < 1 {
    return Err("Minimum session duration must be >= 1 ms".to_string());
}

if self.max_session_duration_ms < self.min_session_duration_ms {
    return Err("Maximum session duration must be >= minimum".to_string());
}

// duration_sec and min_sessions can be 0 (meaning disabled/indefinite)

### `src/generator.rs`

**GeneratorConfig:**
```rust
pub struct GeneratorConfig {
    // ... existing fields
    pub session_prob: f64,
    pub min_session_duration_ms: u64,
    pub max_session_duration_ms: u64,
}
```

**ProcessSimulator:**
```rust
pub struct ProcessSimulator {
    // ... existing fields
    session_prob: f64,
    min_session_duration_ms: u64,
    max_session_duration_ms: u64,
    active_sessions: HashMap<String, String>, // session_type -> session_id
}

impl ProcessSimulator {
    pub fn new(
        // ... existing params
        session_prob: f64,
        min_session_duration_ms: u64,
        max_session_duration_ms: u64,
    ) -> Self {
        Self {
            // ... existing fields
            session_prob,
            min_session_duration_ms,
            max_session_duration_ms,
            active_sessions: HashMap::new(),
        }
    }
    
    fn can_start_session(&self, session_type: &str) -> bool {
        !self.active_sessions.contains_key(session_type)
    }
    
    fn start_session(&mut self, session_type: String, session_id: String) {
        self.active_sessions.insert(session_type, session_id);
    }
    
    fn end_session(&mut self, session_type: &str) {
        self.active_sessions.remove(session_type);
    }
    
    fn random_session_duration(&mut self) -> u64 {
        // Log-uniform distribution for realistic session lengths
        let min_log = (self.min_session_duration_ms as f64).ln();
        let max_log = (self.max_session_duration_ms as f64).ln();
        let random_log = self.rng.gen_range(min_log..=max_log);
        (random_log.exp() as u64) * 1_000_000 // Convert to nanoseconds
    }
    
    pub fn next_message(&mut self) -> Option<Message> {
        // Check pending session ends
        if let Some(msg) = self.check_pending_session_end() {
            return Some(msg);
        }
        
        // Try to start new session
        if self.rng.gen_bool(self.session_prob) {
            // Filter to available session types
            let available: Vec<_> = self.session_types
                .iter()
                .filter(|t| self.can_start_session(t))
                .collect();
            
            if !available.is_empty() {
                let session_type = available[self.rng.gen_range(0..available.len())].clone();
                let session_id = self.generate_id();
                
                // Mark as active
                self.start_session(session_type.clone(), session_id.clone());
                
                // Calculate duration using log-uniform distribution
                let duration_ns = self.random_session_duration();
                
                // Schedule end and emit start...
            }
        }
        
        // Generate event...
    }
    
    fn check_pending_session_end(&mut self) -> Option<Message> {
        if let Some(pending) = self.pending_sessions.peek() {
            if pending.end_time_ns <= self.synthetic_time_ns {
                let pending = self.pending_sessions.pop().unwrap();
                
                // Remove from active sessions
                self.end_session(&pending.session_type);
                
                // Emit session end...
            }
        }
        None
    }
}
```

### `src/delivery.rs`

**MultiProcessGenerator::new:**
```rust
pub fn new(config: GeneratorConfig) -> Self {
    let processes: Vec<ProcessSimulator> = (0..config.num_processes)
        .map(|pid| {
            ProcessSimulator::new(
                pid,
                config.seed,
                config.start_time_ns,
                config.event_names.clone(),
                config.session_types.clone(),
                config.jitter_ms,
                config.mean_delay_ms,
                config.session_prob,
                config.min_session_duration_ms,
                config.max_session_duration_ms,
            )
        })
        .collect();
    // ...
}
```

### `src/main.rs`

**Major rewrite of main loop** (see pseudocode above)

**Key changes:**
1. Remove count-based loop
2. Add ExitState tracking
3. Implement duration and min sessions checking
4. Add drain mode logic
5. Track sessions per process

## Testing Strategy

### Unit Tests

#### `src/generator.rs`

1. **test_session_probability_affects_generation**:
   - Create two processes with different session_prob (0.01 vs 0.5)
   - Generate 1000 messages from each
   - Verify higher probability process generates more sessions

2. **test_process_simulator_accepts_session_prob**:
   - Create ProcessSimulator with custom session_prob
   - Verify it can be constructed and generates messages

3. **test_single_active_session_per_type**:
   - Create process with session types ["a", "b"]
   - Generate many messages
   - Verify no overlapping sessions of same type
   - Track active sessions and ensure constraint holds

4. **test_different_types_can_overlap**:
   - Create process with multiple session types
   - Generate messages and track active sessions
   - Verify sessions of different types can be active simultaneously

5. **test_session_type_reuse_after_end**:
   - Start a session of type "browsing"
   - Wait for it to end
   - Verify new "browsing" session can start after end

#### `src/delivery.rs`

1. **test_min_sessions_per_process**:
   - Configure with min_sessions=10, num_processes=3
   - Run until exit condition met
   - Verify each process generated >= 10 sessions

2. **test_duration_respected**:
   - Configure with duration_sec=1, no min sessions
   - Run and measure actual duration
   - Verify runtime is approximately 1 second (+ drain time)

3. **test_mutually_exclusive_flags**:
   - Verify config validation rejects both duration_sec and min_sessions > 0

### Integration Tests

#### `tests/integration_test.rs`

1. **test_duration_flag**:
   - Run simulator with `--duration-sec 2`
   - Measure execution time
   - Verify it runs for ~2 seconds

2. **test_min_sessions_flag**:
   - Run with `--min-sessions 20 --num-processes 2`
   - Capture output and count sessions per process
   - Verify each process has >= 20 sessions

3. **test_mutually_exclusive**:
   - Run with both `--duration-sec 5 --min-sessions 50`
   - Verify error message and exit

4. **test_indefinite_run**:
   - Run with no duration or min sessions
   - Send Ctrl+C after 1 second
   - Verify clean shutdown with drain

5. **test_session_duration_range**:
   - Run with `--min-session-duration-ms 100 --max-session-duration-ms 500`
   - Verify all session durations within range

## Edge Cases

### 1. Mutually Exclusive Flags

**Scenario:** `--duration-sec 10 --min-sessions 100`

**Expected behavior:**
- Config validation fails
- Error: "Cannot specify both --duration-sec and --min-sessions (mutually exclusive)"
- Exit with error code

### 2. Very High Session Count

**Scenario:** `--min-sessions 10000 --session-types browsing`

**Expected behavior:**
- With 1 session type and realistic durations (e.g., 30s avg)
- Max sessions = (1 * runtime) / 30s
- To get 10000 sessions, need ~300,000 seconds (83 hours)
- Will take very long to complete
- No warning (user explicitly requested this)
- Consider adding progress logging for long runs

### 3. Zero Values (Indefinite Mode)

**Scenario:** No flags specified (or both zero)

**Expected behavior:**
- Run indefinitely until Ctrl+C
- Use default session probability (0.1)
- Use default duration range (1s to 1 hour)

### 4. Session Probability Too Low

**Scenario:** `--min-sessions 100 --session-prob 0.001`

**Expected behavior:**
- Respect user-specified probability
- Will take long time to reach 100 sessions
- With 1 type, 30s avg duration: ~30,000 seconds (8+ hours)
- User is responsible for reasonable configuration

### 5. Drain Timeout

**Scenario:** Many pending sessions when exit condition met

**Expected behavior:**
- Drain all pending sessions (no timeout)
- Sessions scheduled far in future will be emitted during drain
- This is correct behavior - we want to complete all started sessions

### 6. Ctrl+C During Execution

**Scenario:** User presses Ctrl+C before conditions met

**Expected behavior:**
- Immediately transition to drain mode
- Emit all pending session ends
- Exit cleanly
- Do not enforce min_sessions requirement (user explicitly requested stop)

### 7. All Session Types Busy

**Scenario:** Process has session types ["a", "b"], both have active sessions, session generation triggered

**Expected behavior:**
- Skip session generation (no error)
- Fall through to event generation
- This reduces effective session rate, which is acceptable
- Auto-tuning may need to account for this in future

### 8. Single Session Type

**Scenario:** Only one session type configured, e.g., ["browsing"]

**Expected behavior:**
- Sessions are strictly sequential (no overlap)
- Must wait for session to end before starting next
- Session generation rate limited by session duration
- Auto-tuning formula may overestimate achievable rate
- User should configure multiple session types for higher throughput

## Migration Guide

### Before (with --count)

```bash
# Generate exactly 1000 messages
telemetry-sim --count 1000 --num-processes 2
```

### After

**Option 1: Duration mode (realistic simulation)**
```bash
# Run for 10 seconds
telemetry-sim --duration-sec 10 --num-processes 2

# Run for 5 seconds with custom session rate
telemetry-sim --duration-sec 5 --session-prob 0.2 --num-processes 2

# Run indefinitely (Ctrl+C to stop)
telemetry-sim --num-processes 2
```

**Option 2: Count mode (testing)**
```bash
# Generate 100 sessions per process, then exit
telemetry-sim --min-sessions 100 --num-processes 2
```

### Approximate Conversion

If you were using `--count N`, approximate equivalent:

**For duration mode:**
```
messages_per_sec ≈ 1000 * num_processes
duration_sec ≈ N / (1000 * num_processes)

Example:
--count 10000 --num-processes 2
≈ --duration-sec 5 (10000 / 2000 = 5)
```

**For count mode:**
Count sessions instead of messages. Rough estimate:
```
sessions ≈ messages / 10  (assuming 10% session probability)

Example:
--count 10000 --num-processes 2
≈ --min-sessions 500 (10000 / 10 / 2 = 500 per process)
```

## Implementation Phases

### Phase 1: Config Changes
1. Remove `count` field from Config
2. Add `duration_sec`, `min_sessions`, `session_prob`, `min_session_duration_ms`, `max_session_duration_ms` fields
3. Add validation logic (including mutual exclusivity check)
4. Update tests

### Phase 2: Generator Updates
1. Add `session_prob` to `GeneratorConfig`
2. Add `session_prob` to `ProcessSimulator`
3. Add `active_sessions: HashMap<String, String>` to `ProcessSimulator`
4. Update `ProcessSimulator::new()` signature
5. Replace hardcoded `0.1` with `self.session_prob`
6. Implement session type constraint logic:
   - Add `can_start_session()`, `start_session()`, `end_session()` methods
   - Filter available session types before selection
   - Update active_sessions on session start/end
7. Update tests

### Phase 3: Delivery Updates
1. Update `MultiProcessGenerator::new()` to pass session_prob
2. No other changes needed

### Phase 4: Main Loop Rewrite
1. Remove count-based logic
2. Implement `ExitState` struct
3. Implement duration checking
4. Implement min sessions checking
5. Implement drain mode
6. Add session tracking per process
7. Add summary output

### Phase 5: Tests
1. Update existing tests (remove count references)
2. Add unit tests for session probability
3. Add integration tests for duration and min sessions
4. Test edge cases

### Phase 6: Documentation
1. Update SOCKET_API.md
2. Update help text
3. Update README if exists

## Success Criteria

1. All existing tests updated and passing
2. New tests for duration and min sessions passing
3. Manual testing:
   - `--duration-sec 2` runs for ~2 seconds
   - `--min-sessions 10 --num-processes 2` generates >= 20 sessions total (10 per process)
   - Both flags together rejected with error
   - No flags runs indefinitely
   - Ctrl+C triggers clean drain and exit
   - Session type constraints enforced (no overlapping same-type sessions)
4. Session durations respect min/max configuration
5. Documentation updated
6. No performance regression

## Open Questions

1. Should we log progress periodically (e.g., every 10 seconds or every N sessions)?
2. Should we add metrics output at end (sessions/sec, messages/sec, avg session duration, etc.)?
3. Should we add a `--max-duration-sec` safety limit for count mode to prevent runaway execution?
4. Should we support session duration distributions other than log-uniform (e.g., exponential, normal)?
