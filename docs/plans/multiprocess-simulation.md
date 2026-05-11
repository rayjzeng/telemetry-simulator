# Telemetry Simulator Enhancement Plan

## Overview

This document outlines the implementation plan for enhancing the telemetry simulator to support multi-process simulation with data loss, atomic session pairs, and additional configuration options.

## Goals

1. Simulate `n` independent processes writing to the same Unix socket
2. Add configurable data loss via drop rate
3. Generate atomic session start/stop pairs with randomized durations
4. Add process identification and sequence numbering for loss detection
5. Support configurable start time for synthetic clock

## Configuration Changes

### New CLI Arguments (`src/config.rs`)

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `--start-time-ms` | u64 | 0 | Unix timestamp in milliseconds to start synthetic clock |
| `--drop-rate` | f64 | 0.0 | Probability (0.0-1.0) to drop each message after generation |
| `--num-processes` | u32 | 1 | Number of independent processes to simulate |

### Validation Rules

- `start_time_ms`: Must be >= 0 (0 means start from 0)
- `drop_rate`: Must be between 0.0 and 1.0 inclusive
- `num_processes`: Must be >= 1 and <= 1000 (reasonable upper bound)

## Message Schema Changes

### Updated Message Format (`src/message.rs`)

Both `Event` and `Session` variants will include two new fields:

```rust
pub enum Message {
    Event {
        event_id: String,
        event_name: String,
        timestamp_ns: u64,
        version: u8,
        process_id: u32,        // NEW: Process identifier
        sequence_number: u64,   // NEW: Per-process monotonic counter
    },
    Session {
        session_id: String,
        session_type: String,
        timestamp_ns: u64,
        is_start: bool,
        version: u8,
        process_id: u32,        // NEW: Process identifier
        sequence_number: u64,   // NEW: Per-process monotonic counter
    },
}
```

### Field Semantics

- **process_id**: Unique identifier (0 to num_processes-1) for the simulated process
- **sequence_number**: Per-process monotonically increasing counter starting from 0. Gaps indicate dropped messages.

## Generator Architecture Redesign

### Core Components (`src/generator.rs`)

#### 1. ProcessSimulator Struct

Encapsulates state for a single simulated process:

```rust
struct ProcessSimulator {
    process_id: u32,
    rng: StdRng,
    sequence_number: u64,
    synthetic_time_ns: u64,
    config: ProcessConfig,
    pending_sessions: BinaryHeap<Reverse<PendingSession>>,
    active_session_count: usize,
}

struct ProcessConfig {
    event_names: Vec<String>,
    session_types: Vec<String>,
    jitter_ms: f64,
    mean_delay_ms: f64,
}

struct PendingSession {
    end_time_ns: u64,
    session_id: String,
    session_type: String,
}
```

#### 2. MultiProcessGenerator Struct

Coordinates multiple process simulators:

```rust
pub struct MultiProcessGenerator {
    processes: Vec<ProcessSimulator>,
    drop_rate: f64,
    drop_rng: StdRng,  // Separate RNG for drop decisions
    interleave_prob: f64,
    mean_delay_ns: f64,
    delivery_heap: BinaryHeap<Reverse<PendingDelivery>>,
}

struct PendingDelivery {
    delivery_time_ns: u64,
    event_time_ns: u64,
    message: Option<Message>,  // None indicates dropped message
    process_id: u32,
}
```

### Session Generation Logic

#### Current Behavior
- Sessions start when `active_sessions` is empty or with 10% probability
- Sessions end with 30% probability when active sessions exist
- No guarantee of session pairs

#### New Behavior: Atomic Pairs

1. **Session Start Decision**:
   - With configurable probability, start a new session
   - Generate unique `session_id`
   - Select random `session_type`
   - Emit Session(start) message immediately

2. **Duration Calculation**:
   - `min_duration_ns = 10 * mean_delay_ms * 1_000_000`
   - `max_duration_ns = 10000 * mean_delay_ms * 1_000_000`
   - `duration_ns = rng.gen_range(min_duration_ns..=max_duration_ns)`
   - If `mean_delay_ms == 0`, use default range: 10ms to 10s

3. **Schedule Session End**:
   - Calculate `end_time_ns = current_time_ns + duration_ns`
   - Push to `pending_sessions` heap
   - Do NOT track in active_sessions list (events don't require active sessions)

4. **Session End Emission**:
   - Before generating each message, check if any pending sessions have `end_time_ns <= current_time_ns`
   - If so, pop and emit Session(end) message
   - Session end is independent - may be dropped separately from start

### Event Generation Logic

#### Current Behavior
- Events only generated when `!active_sessions.is_empty()`
- 80% probability when active sessions exist

#### New Behavior
- Events can be generated regardless of session state
- Configurable event generation probability (default: maintain similar rate)
- Events are independent of session lifecycle

### Multi-Process Interleaving

#### Process Selection Strategy

**Option A: Round-Robin with Random Skip**
```rust
// Simple deterministic interleaving
current_process = (current_process + 1) % num_processes
// Occasionally skip to simulate bursty behavior
if rng.gen_bool(0.1) {
    current_process = rng.gen_range(0..num_processes);
}
```

**Option B: Weighted Random Selection**
```rust
// Each process has equal probability
process_idx = rng.gen_range(0..num_processes);
```

**Recommendation**: Start with Option B (weighted random) for simplicity and better statistical mixing.

#### Clock Management

Each process maintains independent `synthetic_time_ns`:
- Initialized to `start_time_ms * 1_000_000`
- Advances by `MIN_STEP_NS (1ms) + jitter` per message
- Jitter calculated per-process using process-specific RNG

### Drop Simulation

#### Implementation

```rust
fn maybe_drop(&mut self, msg: Message) -> Option<Message> {
    if self.drop_rate > 0.0 && self.drop_rng.gen_bool(self.drop_rate) {
        // Message is dropped but sequence number is still consumed
        None
    } else {
        Some(msg)
    }
}
```

#### Sequence Number Handling

1. Increment sequence number BEFORE drop decision
2. Assign sequence number to message
3. Apply drop decision
4. If dropped, message is discarded but gap remains in sequence

#### Example

Process 0 with drop_rate = 0.1:
```
seq 0: Event -> sent
seq 1: Session(start) -> dropped (gap)
seq 2: Event -> sent
seq 3: Event -> sent
seq 4: Session(end) -> dropped (gap)
seq 5: Event -> sent
```

Receiver sees sequences: 0, 2, 3, 5 (detects drops at 1 and 4)

## Delivery Queue with Drops

### Modified DeliveryQueue

The delivery queue must handle `None` messages (dropped):

```rust
struct PendingDelivery {
    delivery_time_ns: u64,
    event_time_ns: u64,
    message: Option<Message>,  // None = dropped
    process_id: u32,
    sequence_number: u64,
}
```

### Drain Behavior

When draining:
- Pop all pending deliveries
- Emit only `Some(message)` entries
- Dropped messages (`None`) are silently discarded
- This simulates messages lost in transit

## Main Loop Changes (`src/main.rs`)

### Configuration Construction

```rust
let gen_config = GeneratorConfig {
    seed: config.seed,
    event_names: config.get_event_names(),
    session_types: config.get_session_types(),
    jitter_ms: config.jitter_ms,
    interleave_prob: config.interleave_prob,
    mean_delay_ms: config.mean_delay_ms,
    start_time_ns: config.start_time_ms * 1_000_000,
    drop_rate: config.drop_rate,
    num_processes: config.num_processes,
};
```

### Message Handling

```rust
while !shutdown.load(Ordering::Relaxed) {
    let msg_opt = queue.next_message();
    
    match msg_opt {
        Some(msg) => {
            // Message generated and not dropped
            if let Err(e) = write_msg(&mut output, &msg, use_stdout).await {
                eprintln!("Write error: {}", e);
                return ExitCode::from(4);
            }
            sent_count += 1;
        }
        None => {
            // Either no message ready or message was dropped
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
            continue;
        }
    }
}
```

## API Documentation Updates (`docs/SOCKET_API.md`)

### New Fields Section

Add documentation for `process_id` and `sequence_number` fields in both Event and Session messages.

### Configuration Impact Table

Add new flags:

| Flag | Impact on Messages |
|------|-------------------|
| `--start-time-ms` | Sets initial timestamp value (default: 0) |
| `--drop-rate` | Probability of message loss; creates gaps in sequence numbers |
| `--num-processes` | Number of simulated processes; messages include process_id |

### Data Loss Detection

Add section explaining how to detect data loss using sequence numbers:

```
Data Loss Detection:
Each process maintains an independent sequence counter. To detect loss:
1. Track last seen sequence_number per process_id
2. If current sequence_number > last + 1, messages were dropped
3. Gap size = current - last - 1
```

## Testing Strategy

### Unit Tests (`src/generator.rs`)

1. **test_multi_process_deterministic**:
   - Create generator with num_processes=3, seed=42
   - Generate 100 messages from two identical generators
   - Verify sequences match exactly

2. **test_session_pairs_generated**:
   - Generate messages with session activity
   - Verify every session start has corresponding end (when not dropped)
   - Verify end timestamp > start timestamp

3. **test_session_duration_range**:
   - Generate 1000 session pairs
   - Verify all durations within [10*mean_delay_ms, 10000*mean_delay_ms]

4. **test_events_without_sessions**:
   - Configure with high event rate, low session rate
   - Verify events are generated even with no active sessions

5. **test_sequence_numbers_monotonic**:
   - For each process_id, collect sequence numbers
   - Verify strictly increasing by 1 (when no drops)

6. **test_drop_rate_creates_gaps**:
   - Set drop_rate=0.2, generate 1000 messages
   - Verify gaps exist in per-process sequences
   - Verify gap frequency approximates drop_rate

7. **test_per_process_isolation**:
   - Verify process 0 and process 1 have independent:
     - Sequence counters
     - RNG streams
     - Clocks

### Integration Tests (`tests/integration_test.rs`)

1. **test_multi_process_output**:
   - Start simulator with `--num-processes 3`
   - Verify messages contain process_id 0, 1, 2
   - Verify each process has monotonic sequences

2. **test_drop_simulation**:
   - Start with `--drop-rate 0.1 --count 1000`
   - Verify received count < 1000 (approximately 900)
   - Verify sequence gaps in output

3. **test_start_time**:
   - Start with `--start-time-ms 1640995200000`
   - Verify first timestamp >= 1640995200000000000 ns

## Implementation Phases

### Phase 1: Message Schema & Config
1. Update `src/message.rs` with new fields
2. Update `src/config.rs` with new CLI args
3. Update serialization tests
4. Update documentation

### Phase 2: ProcessSimulator
1. Create `ProcessSimulator` struct
2. Implement session pair generation
3. Implement event generation (session-independent)
4. Add sequence numbering
5. Unit tests for single process

### Phase 3: Multi-Process Coordination
1. Create `MultiProcessGenerator` struct
2. Implement process selection logic
3. Implement interleaving
4. Unit tests for multi-process

### Phase 4: Drop Simulation
1. Add drop decision logic
2. Integrate with delivery queue
3. Handle None messages in queue
4. Unit tests for drop behavior

### Phase 5: Integration
1. Update `src/main.rs` to use new generator
2. Update `src/lib.rs` exports
3. Integration tests
4. Documentation updates

### Phase 6: Validation
1. Run full test suite
2. Verify determinism with fixed seeds
3. Benchmark performance impact
4. Update README if needed

## Backward Compatibility

### Default Behavior
With default arguments (`num_processes=1`, `drop_rate=0.0`, `start_time_ms=0`):
- Single process simulation (process_id always 0)
- No message drops
- Clock starts at 0
- Behavior similar to current implementation (except session/event logic changes)

### Breaking Changes
1. **Session/Event Coupling**: Events no longer require active sessions
2. **Session Generation**: Sessions are now generated as atomic pairs with scheduled ends
3. **Message Format**: New required fields (`process_id`, `sequence_number`)

Consumers must update to handle new fields. The version field remains 1, but this is a semantic breaking change.

## Performance Considerations

### Memory Usage
- `num_processes` × (RNG state + pending sessions heap)
- Pending sessions: O(active sessions) per process
- Delivery queue: O(delayed messages) total

### CPU Usage
- Process selection: O(1) per message
- Session scheduling: O(log S) where S = pending sessions
- Drop decision: O(1) per message

Expected overhead is minimal for reasonable `num_processes` (< 100).

## Determinism Guarantees

With fixed `--seed`:
1. Process RNGs seeded as `seed + process_id` (or similar deterministic scheme)
2. Drop RNG seeded as `seed + 0xFFFF` (or similar)
3. Process selection uses deterministic RNG
4. Same seed produces identical:
   - Message sequence
   - Process interleaving
   - Drop pattern
   - Session durations

## Open Questions

1. Should we add `--event-prob` to control event generation rate independently?
2. Should session start probability be configurable?
3. Do we need to expose process selection strategy as config?
4. Should we support process-specific configurations (different event names per process)?

## Migration Guide for Consumers

### Before (v1)
```json
{
  "type": "event",
  "event_id": "...",
  "event_name": "click",
  "timestamp_ns": 1000000,
  "version": 1
}
```

### After (v1 with new fields)
```json
{
  "type": "event",
  "event_id": "...",
  "event_name": "click",
  "timestamp_ns": 1000000,
  "version": 1,
  "process_id": 0,
  "sequence_number": 42
}
```

### Detection Logic
```python
# Track per-process sequences
last_seq = defaultdict(lambda: -1)

def process_message(msg):
    pid = msg['process_id']
    seq = msg['sequence_number']
    
    if last_seq[pid] >= 0:
        expected = last_seq[pid] + 1
        if seq > expected:
            dropped = seq - expected
            print(f"Process {pid}: dropped {dropped} messages")
    
    last_seq[pid] = seq
```

## Success Criteria

1. All existing tests pass (with updates for new fields)
2. New unit tests achieve > 90% coverage of new code
3. Integration tests verify end-to-end functionality
4. Determinism: Same seed produces identical output across runs
5. Performance: No > 10% regression in messages/sec for single-process mode
6. Documentation: SOCKET_API.md fully updated
7. Drop simulation: Observed drop rate within 5% of configured rate over 10k messages
