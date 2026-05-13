# Telemetry Simulator - Agent Guide

## Prerequisites

Santa (macOS endpoint security) must be in Monitor mode for the Rust toolchain and compiled binaries to execute. Verify before building:

```bash
santactl status | grep -i mode  # Must show "Monitor"
```

If Santa is in Lockdown mode, builds and test runs will be blocked.

## Build Commands

### Build
```bash
cargo build                    # Debug build
cargo build --release          # Release build (optimized)
cargo build --features stdout  # Build with stdout/JSON output features
```

### Linting
```bash
cargo clippy                   # Run clippy linter
cargo clippy -- -W clippy::pedantic  # Stricter linting
cargo fmt -- --check           # Check formatting without modifying
cargo fmt                      # Format code
```

### Testing

#### Run All Tests
```bash
cargo test                     # Run all tests (unit + integration)
cargo test --lib               # Run only library unit tests
cargo test --test integration_test  # Run integration tests only
```

#### Run Single Test
```bash
cargo test test_name           # Run test by name (fuzzy match)
cargo test test_event_serialization_roundtrip  # Run specific unit test
cargo test test_burst_mode_generates_exact_count  # Run specific integration test
cargo test -- --nocapture      # Show println! output
cargo test -- --test-threads=1 # Run tests serially
```

#### Test with Features
```bash
cargo test --features stdout   # Run tests with stdout feature enabled
```

## Code Style Guidelines

### Imports

Group imports in this order with blank lines between groups:
1. Standard library imports
2. External crate imports
3. Internal module imports

```rust
use std::collections::BinaryHeap;
use std::sync::Arc;

use clap::Parser;
use rand::rngs::StdRng;
use tokio::io::AsyncWriteExt;

use crate::message::Message;
use crate::generator::GeneratorConfig;
```

### Formatting

- **Line length**: Aim for 100 characters max
- **Indentation**: 4 spaces (standard Rust)
- **Braces**: Same line for functions, structs, enums (K&R style)
- **Trailing commas**: Required in multi-line struct/enum definitions

```rust
pub struct Config {
    field_one: String,
    field_two: u64,
    field_three: Option<Vec<String>>,
}
```

### Types

#### Naming Conventions
- **Structs/Enums**: `PascalCase` (e.g., `ProcessSimulator`, `Message`)
- **Functions/Methods**: `snake_case` (e.g., `next_message`, `current_time_ns`)
- **Constants**: `SCREAMING_SNAKE_CASE` (e.g., `MAX_SOCKET_PATH`)
- **Type parameters**: Single uppercase letter or `PascalCase` (e.g., `T`, `W`)
- **Modules**: `snake_case` (e.g., `delivery`, `generator`)

#### Type Annotations
- Prefer explicit return types for public functions
- Use type inference for locals when obvious
- Use `u64` for timestamps (nanoseconds)
- Use `u32` for process IDs and counts
- Use `f64` for probabilities and rates

```rust
pub fn current_time_ns(&self) -> u64 {
    // Implementation
}

pub fn validate(&self) -> Result<(), String> {
    // Implementation
}
```

#### Generics
Use trait bounds for async I/O:

```rust
pub async fn write_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &Message,
) -> Result<(), std::io::Error> {
    // Implementation
}
```

### Error Handling

#### Result Types
- Use `Result<T, E>` for fallible operations
- Prefer specific error types over `String` when possible
- For simple validation, `Result<(), String>` is acceptable

```rust
pub fn validate(&self) -> Result<(), String> {
    if self.socket.len() > 107 {
        return Err("Socket path too long (max 107 characters)".to_string());
    }
    Ok(())
}
```

#### Error Propagation
- Use `?` operator for error propagation
- Use `.map_err()` to convert error types

```rust
let payload = msg.to_msgpack().map_err(|e| {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
})?;
```

#### Panics
- Avoid panics in library code
- Use `expect()` with descriptive messages in tests or main
- Use `unwrap()` only when logically impossible to fail

```rust
// Good - in tests with clear intent
let serialized = msg.to_msgpack().unwrap();

// Better - with context
let child = Command::new("cargo")
    .spawn()
    .expect("Failed to start simulator");
```

### Async Code

#### Runtime
- Use `#[tokio::main]` for binary entry point
- Use `#[tokio::test]` for async tests

#### Async Functions
```rust
async fn write_msg<W: tokio::io::AsyncWrite + Unpin>(
    output: &mut W,
    msg: &Message,
) -> Result<(), std::io::Error> {
    // Implementation
}
```

#### Spawning Tasks
```rust
tokio::spawn(async move {
    let _ = signal::ctrl_c().await;
    shutdown_clone.store(true, Ordering::Relaxed);
});
```

### Structs and Enums

#### Struct Definition
```rust
#[derive(Clone)]
pub struct GeneratorConfig {
    pub seed: Option<u64>,
    pub event_names: Vec<String>,
    pub session_types: Vec<String>,
    pub interleave_prob: f64,
    pub mean_delay_ms: f64,
    // ...
}
```

#### Enum Definition with Serde
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    #[serde(rename = "event")]
    Event {
        event_id: String,
        event_name: String,
        timestamp_ns: u64,
        version: u8,
        process_id: u32,
        sequence_number: u64,
    },
    #[serde(rename = "session")]
    Session {
        session_id: String,
        session_type: String,
        timestamp_ns: u64,
        is_start: bool,
        version: u8,
        process_id: u32,
        sequence_number: u64,
    },
}
```

#### Implementation Blocks
Group methods logically and use `impl` blocks:

```rust
impl Message {
    pub fn to_msgpack(&self) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec_named(self)
    }

    pub fn from_msgpack(data: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        rmp_serde::from_slice(data)
    }
}
```

### Documentation

#### Module Documentation
```rust
//! This module handles message delivery with simulated network delays.
```

#### Function Documentation
```rust
/// Generates the next message in the stream.
///
/// Returns `None` when draining is complete and no messages remain.
/// Messages may be delayed based on configured interleave probability.
pub fn next_message(&mut self) -> Option<Message> {
    // Implementation
}
```

#### Inline Comments
- Explain "why", not "what"
- Use for complex algorithms or non-obvious logic

```rust
// Reverse for min-heap (BinaryHeap is max-heap by default)
other.time_ns.cmp(&self.time_ns)
```

### Testing

#### Unit Tests
Place tests in same file within `#[cfg(test)]` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_serialization_roundtrip() {
        let msg = Message::Event {
            event_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            event_name: "click".to_string(),
            timestamp_ns: 1000000,
            version: 1,
            process_id: 0,
            sequence_number: 42,
        };
        let serialized = msg.to_msgpack().unwrap();
        let deserialized = Message::from_msgpack(&serialized).unwrap();
        assert_eq!(msg, deserialized);
    }
}
```

#### Async Tests
```rust
#[tokio::test]
async fn test_writer_frames_with_length_prefix() {
    let mut buf = Vec::new();
    let msg = Message::Event { /* ... */ };
    write_message(&mut buf, &msg).await.unwrap();
    
    let frame_len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    assert_eq!(frame_len, buf.len() - 4);
}
```

#### Integration Tests
Located in `tests/` directory, use `mod common` for shared utilities.

### Configuration

#### Clap Derive
Use derive macros for CLI configuration:

```rust
#[derive(Parser, Debug, Clone)]
#[command(name = "telemetry-sim")]
pub struct Config {
    #[arg(short, long, default_value = "/tmp/telemetry.sock")]
    pub socket: String,

    #[arg(long, default_value = "0.0")]
    pub mean_delay_ms: f64,

    #[arg(long)]
    pub seed: Option<u64>,
}
```

### Feature Flags

Use `#[cfg(feature = "...")]` for conditional compilation:

```rust
#[cfg(feature = "stdout")]
#[arg(long)]
stdout: bool,

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
```

## Project Structure

```
telemetry-simulator/
├── Cargo.toml              # Package manifest and dependencies
├── Cargo.lock              # Locked dependency versions
├── src/
│   ├── lib.rs             # Library root, module declarations
│   ├── main.rs            # Binary entry point
│   ├── config.rs          # CLI configuration (clap)
│   ├── message.rs         # Message types (Event, Session)
│   ├── generator.rs       # Process simulation and message generation
│   ├── delivery.rs        # Multi-process coordination and delivery queue
│   └── writer.rs          # Socket writing with length-prefix framing
├── tests/
│   ├── common/mod.rs      # Test utilities (TestSimulator)
│   └── integration_test.rs # Integration tests
└── docs/
    ├── SOCKET_API.md      # Socket protocol specification
    └── plans/             # Design documents
```

## Key Concepts

### Message Types
- **Event**: User interaction (click, view, purchase, etc.)
- **Session**: Session start/end markers

### Process Simulation
- Each process has independent clock, RNG, and sequence numbers
- Messages interleaved from multiple processes
- Sequence numbers are per-process (not global)

### Delivery Semantics
- **interleave_prob**: Probability of message delay
- **mean_delay_ms**: Average delay for delayed messages
- **drop_rate**: Probability of message loss (creates sequence gaps)

### Output Formats
- **Socket mode** (default): Unix domain socket with MessagePack + length prefix
- **Stdout mode** (`--features stdout`): JSON to stdout
- **JSON file** (`--features stdout --json path`): JSON array to file

## Running the Simulator

```bash
# Basic usage (waits for client on /tmp/telemetry.sock)
cargo run

# With duration limit
cargo run -- --duration-sec 10

# With session count limit
cargo run -- --min-sessions 100

# With stdout output (requires stdout feature)
cargo run --features stdout -- --stdout --duration-sec 5

# JSON file output
cargo run --features stdout -- --json /tmp/output.json --min-sessions 10

# Multi-process simulation
cargo run -- --num-processes 4 --duration-sec 10

# With delays and drops
cargo run -- --interleave-prob 0.1 --mean-delay-ms 100 --drop-rate 0.05
```

## Dependencies

- **tokio**: Async runtime, networking, signals
- **clap**: CLI argument parsing (derive feature)
- **serde**: Serialization framework (derive feature)
- **serde_json**: JSON serialization
- **rmp-serde**: MessagePack serialization
- **uuid**: UUID generation (v4)
- **rand**: Random number generation (StdRng)

Dev dependencies:
- **tempfile**: Temporary files in tests
- **assert_matches**: Pattern matching assertions
