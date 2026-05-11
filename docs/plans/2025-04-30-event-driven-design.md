# Design: Event-Driven Telemetry Simulator

**Goal**: Replace timestep-based generation with event-driven architecture using Poisson processes for realistic, efficient message generation with session type constraints.

**Architecture**: Event-driven simulation with priority queue scheduling, Poisson processes for arrivals, log-uniform session durations, and per-type session exclusivity.

**Tech Stack**: Rust, rand crate (StdRng, Exponential), BinaryHeap for event queue

## Overview

The current timestep-based simulator advances a synthetic clock by fixed increments (1ms + jitter) and uses probabilities to decide whether to generate messages. This is inefficient and doesn't accurately model inter-arrival times.

The new design uses event-driven simulation:
- Schedule next event time using exponential distribution (Poisson process)
- Jump directly to next event time (no wasted iterations)
- Separate Poisson processes for sessions and events
- Session type constraints enforced (1 active per type)
- Realistic log-uniform session durations (seconds to hours)

## Requirements

1. **Event-driven**: Schedule events at specific times, jump to next event
2. **Poisson processes**: Exponential inter-arrival times for sessions and events
3. **Session constraints**: Only 1 active session per session_type per process
4. **Uniform types**: Session types selected uniformly from available types
5. **Correlated events**: Events only generated when sessions active, rate scales with active count
6. **Realistic durations**: Log-uniform distribution from seconds to hours
7. **Mutually exclusive modes**: `--duration-sec` OR `--min-sessions`, not both

## Architecture

### Components

```
┌─────────────────────────────────────────────────────────┐
│           MultiProcessGenerator                          │
│  - Priority queue: (time, process_id)                    │
│  - Collects events from ProcessSimulators                │
│  - Returns earliest event                                │
└──────────────────┬──────────────────────────────────────┘
                   │ subscribes to
        ┌──────────┼──────────┬──────────┐
        ▼          ▼          ▼          ▼
┌─────────────┐ ┌─────────────┐      ┌─────────────┐
│ ProcessSim  │ │ ProcessSim  │      │ ProcessSim  │
│     0       │ │     1       │      │     N       │
│             │ │             │      │             │
│ Schedules:  │ │ Schedules:  │      │ Schedules:  │
│ - Sessions  │ │ - Sessions  │      │ - Sessions  │
│ - Events    │ │ - Events    │      │ - Events    │
│ (only when  │ │ (only when  │      │ (only when  │
│  sessions   │ │  sessions   │      │  sessions   │
│  active)    │ │  active)    │      │  active)    │
└─────────────┘ └─────────────┘      └─────────────┘
```

### Key Design Principles

1. **ProcessSimulator is active** - Schedules its own events using Poisson processes
2. **MultiProcessGenerator is passive** - Collects and orders events from processes
3. **Events coupled to sessions** - No events when no active sessions
4. **Session type constraints** - Enforced at session start time
5. **Uniform type selection** - Simple random from available types

## Components

### Event Types (Internal)

```rust
// Internal to ProcessSimulator, not exposed
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

impl Ord for ScheduledEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse for min-heap (earliest time first)
        other.time_ns().cmp(&self.time_ns())
    }
}
```

### ProcessSimulator

```rust
pub struct ProcessSimulator {
    // Identity
    process_id: u32,
    rng: StdRng,
    sequence_number: u64,
    current_time_ns: u64,
    
    // Configuration
    event_names: Vec<String>,
    session_types: Vec<String>,
    session_rate: f64,              // sessions per second
    event_rate: f64,                // events per second per active session
    min_session_duration_ms: u64,
    max_session_duration_ms: u64,
    
    // State
    event_queue: BinaryHeap<Reverse<ScheduledEvent>>,
    active_sessions: HashMap<String, SessionInfo>,
}

struct SessionInfo {
    session_id: String,
    end_time: u64,
}

impl ProcessSimulator {
    pub fn new(
        process_id: u32,
        seed: Option<u64>,
        start_time_ns: u64,
        event_names: Vec<String>,
        session_types: Vec<String>,
        session_rate: f64,
        event_rate: f64,
        min_session_duration_ms: u64,
        max_session_duration_ms: u64,
    ) -> Self {
        let mut sim = Self {
            process_id,
            rng: StdRng::seed_from_u64(
                seed.unwrap_or(0).wrapping_add(process_id as u64)
            ),
            sequence_number: 0,
            current_time_ns: start_time_ns,
            event_names,
            session_types,
            session_rate,
            event_rate,
            min_session_duration_ms,
            max_session_duration_ms,
            event_queue: BinaryHeap::new(),
            active_sessions: HashMap::new(),
        };
        
        // Schedule first session
        sim.schedule_next_session();
        
        sim
    }
    
    /// Get time of next scheduled event, or None if empty
    pub fn peek_next_event_time(&self) -> Option<u64> {
        self.event_queue.peek().map(|e| e.0.time_ns())
    }
    
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
        // Check if type already active (shouldn't happen, but be safe)
        if self.active_sessions.contains_key(&session_type) {
            // Type became busy since we scheduled this
            // Reschedule for later
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
        
        // If this is first session, schedule first event
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
        
        // If no more active sessions, cancel next event
        if self.active_sessions.is_empty() {
            // Clear event queue of EventMessage items
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
            event_name: self.choose_event_name(),
            timestamp_ns: time_ns,
            version: 1,
            process_id: self.process_id,
            sequence_number: self.next_sequence(),
        };
        
        Some((time_ns, msg))
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
            let event = ScheduledEvent::SessionStart {
                time_ns: retry_time,
                session_type: String::new(),
                session_id: String::new(),
            };
            self.event_queue.push(Reverse(event));
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

### MultiProcessGenerator

```rust
pub struct MultiProcessGenerator {
    processes: Vec<ProcessSimulator>,
    // Priority queue of (time, process_id)
    event_queue: BinaryHeap<Reverse<(u64, usize)>>,
    drop_rate: f64,
    drop_rng: StdRng,
}

impl MultiProcessGenerator {
    pub fn new(config: GeneratorConfig) -> Self {
        let mut processes = Vec::new();
        let mut event_queue = BinaryHeap::new();
        
        for pid in 0..config.num_processes {
            let process = ProcessSimulator::new(
                pid,
                config.seed,
                config.start_time_ns,
                config.event_names.clone(),
                config.session_types.clone(),
                config.session_rate,
                config.event_rate,
                config.min_session_duration_ms,
                config.max_session_duration_ms,
            );
            
            // Get process's next event time
            if let Some(next_time) = process.peek_next_event_time() {
                event_queue.push(Reverse((next_time, pid as usize)));
            }
            
            processes.push(process);
        }
        
        let drop_rng = match config.seed {
            Some(s) => StdRng::seed_from_u64(s.wrapping_add(0x20000)),
            None => StdRng::from_entropy(),
        };
        
        Self {
            processes,
            event_queue,
            drop_rate: config.drop_rate,
            drop_rng,
        }
    }
}

impl MessageStream for MultiProcessGenerator {
    fn next_message(&mut self) -> Option<Message> {
        // Get process with earliest event
        let Reverse((_, pid)) = self.event_queue.pop()?;
        let process = &mut self.processes[pid];
        
        // Get event from process
        let (event_time, msg) = process.pop_next_event()?;
        
        // Update process's next event in queue
        if let Some(next_time) = process.peek_next_event_time() {
            self.event_queue.push(Reverse((next_time, pid)));
        }
        
        // Apply drop decision
        if self.drop_rate > 0.0 && self.drop_rng.gen_bool(self.drop_rate) {
            return None;
        }
        
        Some(msg)
    }
}
```

## Configuration

### New Flags

```bash
# Session generation
--session-rate <RATE>               # Sessions per second per process (default: 0.1)
--min-session-duration-ms <MS>      # Min session duration (default: 1000)
--max-session-duration-ms <MS>      # Max session duration (default: 3600000)

# Event generation
--event-rate <RATE>                 # Events per second per active session (default: 2.0)

# Execution mode (mutually exclusive)
--duration-sec <SEC>                # Run for N seconds
--min-sessions <N>                  # Generate N sessions per process

# Session types (uniform selection)
--session-types <TYPES>             # Comma-separated (default: browsing,checkout,search)
```

### Removed Flags

- `--count` - Replaced by `--min-sessions`
- `--session-prob` - Replaced by `--session-rate`
- `--mean-delay-ms` - No longer used for generation (only delivery)
- `--jitter-ms` - No longer used for generation (only delivery)

### Example Configurations

**Realistic web traffic (1 hour):**
```bash
telemetry-sim \
  --duration-sec 3600 \
  --session-rate 0.05 \
  --session-types browsing,search,checkout,profile \
  --event-rate 1.0 \
  --min-session-duration-ms 5000 \
  --max-session-duration-ms 1800000 \
  --num-processes 10
```
- 0.05 sessions/sec = 1 session every 20 seconds per process
- 10 processes = 0.5 sessions/sec = 1800 sessions/hour
- 4 session types, uniform selection
- 1 event/sec per active session
- Sessions 5 sec to 30 min (log-uniform)

**High-throughput testing:**
```bash
telemetry-sim \
  --min-sessions 1000 \
  --session-rate 5.0 \
  --session-types a,b,c,d,e,f,g,h,i,j \
  --event-rate 20.0 \
  --min-session-duration-ms 100 \
  --max-session-duration-ms 5000 \
  --num-processes 5
```
- 5 sessions/sec per process
- 10 types for high concurrency
- Short sessions 100ms-5s
- 20 events/sec per active session
- Generates 5000 sessions total, then exits

## Testing Strategy

### Unit Tests

**ProcessSimulator:**
1. `test_session_scheduling` - Sessions scheduled at Poisson intervals
2. `test_event_scheduling` - Events scheduled correctly, only when sessions active
3. `test_session_type_exclusivity` - No overlapping same-type sessions
4. `test_different_types_overlap` - Different types can overlap
5. `test_event_rate_scales` - More sessions = more events
6. `test_log_uniform_durations` - Durations follow log-uniform distribution
7. `test_uniform_type_selection` - Types selected uniformly from available

**MultiProcessGenerator:**
1. `test_earliest_event_selected` - Correct process chosen
2. `test_processes_independent` - Processes don't interfere
3. `test_deterministic` - Same seed = same output

### Integration Tests

1. **test_duration_mode** - Runs for specified duration
2. **test_min_sessions_mode** - Generates exact session count
3. **test_mutually_exclusive** - Both flags rejected
4. **test_session_constraints** - Type exclusivity held
5. **test_events_during_sessions** - No events without active sessions

### Statistical Tests

1. **test_poisson_interarrivals** - Kolmogorov-Smirnov test for exponential
2. **test_log_uniform** - Chi-square test for log-uniform durations
3. **test_uniform_types** - Chi-square test for uniform type selection

## Migration Guide

### Before (timestep-based)
```bash
telemetry-sim --count 1000 --session-prob 0.1 --mean-delay-ms 10
```

### After (event-driven)
```bash
# For testing (exact count)
telemetry-sim --min-sessions 100 --session-rate 1.0 --num-processes 2

# For realistic simulation
telemetry-sim --duration-sec 60 --session-rate 0.1 --event-rate 2.0
```

### Approximate Conversion

**Count mode:**
```
Old: --count N --num-processes P
New: --min-sessions (N / 10 / P)  # Assuming ~10% of messages are session starts
```

**Duration mode:**
```
Old: --count N --num-processes P
New: --duration-sec (N / 1000 / P)  # Assuming ~1000 msgs/sec per process
```

## Performance Considerations

**Event-driven advantages:**
- No wasted iterations
- O(log P) to select next process (P = num processes)
- O(log E) per process queue ops (E = events in queue, typically 2-3)
- Scales to high rates efficiently

**Expected performance:**
- 100 processes, 10K events/sec: < 5% CPU
- Memory: ~1KB per process

## Success Criteria

1. All 20 existing tests updated and passing
2. New tests for event-driven behavior passing
3. Statistical tests verify Poisson and log-uniform distributions
4. Manual testing:
   - Duration mode runs correct time
   - Min-sessions mode generates exact count
   - Session constraints enforced
   - Events only during sessions
   - Mutually exclusive flags rejected
5. Performance: 10K events/sec with < 10% CPU
6. Documentation updated
