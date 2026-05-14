# Telemetry Simulator

A high-performance telemetry event simulator that generates realistic session and event data over Unix domain sockets. Useful for testing telemetry pipelines, analytics systems, and event-driven architectures.

## Features

- **Multi-Process Simulation**: Simulate multiple independent processes with isolated state
- **Realistic Event Generation**: Configurable event rates with exponential timing distribution
- **Network Simulation**: Optional message reordering and loss to test robustness
- **Deterministic Output**: Seed-based random generation for reproducible tests
- **Multiple Output Formats**: Unix socket (default), stdout JSON, or JSON file output

## Documentation

- [Socket API Specification](docs/SOCKET_API.md) - Output format and protocol

## Configuration Reference

### Overview

The telemetry simulator can be configured through:
1. **Command-line arguments** - For quick overrides and testing
2. **Configuration files** - TOML format for persistent settings
3. **Default values** - Built-in defaults when neither CLI nor file specifies a value

Configuration precedence (highest to lowest):
1. Command-line arguments
2. Configuration file values
3. Built-in defaults

### Command-Line Interface

#### Usage

```bash
telemetry-sim [OPTIONS]
```

#### Options

##### `-c, --config <PATH>`
Path to TOML configuration file.

**Type**: Path  
**Default**: None  
**Example**: `--config /etc/telemetry/config.toml`

If the path is relative, it is resolved relative to the current working directory. Paths in the config file (like `socket` or `json`) are resolved relative to the config file's directory.

---

##### `-s, --socket <PATH>`
Unix domain socket path for output.

**Type**: String  
**Default**: `/tmp/telemetry.sock`  
**Example**: `--socket /var/run/telemetry.sock`

The socket path must be less than 107 characters (Unix domain socket limit). The simulator will remove any existing socket file at this path before binding.

**Config file equivalent**: `socket`

---

##### `--mean-delay-ms <MILLISECONDS>`
Mean network delay for out-of-order delivery simulation.

**Type**: Float  
**Default**: `0.0`  
**Range**: `0.0` to `1,000,000.0`  
**Example**: `--mean-delay-ms 50.0`

When `interleave_prob` > 0, messages are delayed by a random amount sampled from an exponential distribution with this mean. If `interleave_prob` is 0, this setting has no effect.

**Config file equivalent**: `mean_delay_ms`

---

##### `--seed <NUMBER>`
Random seed for deterministic output.

**Type**: Integer (u64)  
**Default**: None (random)  
**Example**: `--seed 42`

When specified, all random number generation uses this seed, producing identical output across runs. Each process gets a derived seed (seed + process_id) to ensure different but deterministic sequences across processes.

**Config file equivalent**: `seed`

---

##### `--duration-sec <SECONDS>`
Run duration in seconds (mutually exclusive with `--min-sessions`).

**Type**: Integer (u64)  
**Default**: `0` (run indefinitely)  
**Example**: `--duration-sec 60`

The simulator runs for this many seconds of simulated time, then enters draining mode to finish pending sessions before exiting.

Cannot be used with `--min-sessions`.

**Config file equivalent**: `duration_sec`

---

##### `--min-sessions <COUNT>`
Minimum sessions per process before exit (mutually exclusive with `--duration-sec`).

**Type**: Integer (u64)  
**Default**: `0` (no minimum)  
**Example**: `--min-sessions 100`

The simulator runs until each process has started at least this many sessions, then enters draining mode.

Cannot be used with `--duration-sec`.

**Config file equivalent**: `min_sessions`

---

##### `--session-gap-mean-ms <MILLISECONDS>`
Mean gap between consecutive sessions.

**Type**: Integer (u64)  
**Default**: `1000` (1 second)  
**Range**: > 0  
**Example**: `--session-gap-mean-ms 5000`

Gap duration follows exponential distribution. This controls how frequently new sessions start.

**Config file equivalent**: `session_gap_mean_ms`

---

##### `--event-rate <RATE>`
Events per second per process.

**Type**: Float  
**Default**: `2.0`  
**Range**: > 0.0  
**Example**: `--event-rate 10.0`

Event inter-arrival times follow exponential distribution with this rate. Higher values produce more events.

**Config file equivalent**: `event_rate`

---

##### `--duration-mean-ms <MILLISECONDS>`
Mean session duration in milliseconds.

**Type**: Integer (u64)  
**Default**: `60000` (60 seconds)  
**Range**: > 0  
**Example**: `--duration-mean-ms 120000`

Session durations follow log-normal distribution with this mean. Applies to all sessions globally.

**Config file equivalent**: `duration_mean_ms`

---

##### `--duration-stddev-ms <MILLISECONDS>`
Standard deviation of session duration in milliseconds.

**Type**: Integer (u64)  
**Default**: `120000` (120 seconds)  
**Range**: > 0  
**Example**: `--duration-stddev-ms 60000`

Used with `duration_mean_ms` to parameterize the log-normal distribution for session durations.

**Config file equivalent**: `duration_stddev_ms`

---

##### `--interleave-prob <PROBABILITY>`
Probability of message reordering (0.0 to 1.0).

**Type**: Float  
**Default**: `0.0`  
**Range**: `0.0` to `1.0`  
**Example**: `--interleave-prob 0.1`

When a message is generated, it has this probability of being delayed by a random amount (exponential with mean `mean_delay_ms`). This simulates network reordering.

If `interleave_prob` > 0 but `mean_delay_ms` is 0, a warning is issued and no reordering occurs.

**Config file equivalent**: `interleave_prob`

---

##### `--start-time-ms <MILLISECONDS>`
Initial synthetic time in milliseconds.

**Type**: Integer (u64)  
**Default**: `0`  
**Example**: `--start-time-ms 1640995200000`

Sets the starting timestamp for generated messages (nanoseconds = milliseconds × 1,000,000). Useful for generating data with specific timestamps.

**Config file equivalent**: `start_time_ms`

---

##### `--drop-rate <PROBABILITY>`
Probability of message loss (0.0 to 1.0).

**Type**: Float  
**Default**: `0.0`  
**Range**: `0.0` to `1.0`  
**Example**: `--drop-rate 0.05`

Each message has this probability of being dropped (not delivered). Dropped messages still consume sequence numbers, creating gaps that test client handling of missing messages.

**Config file equivalent**: `drop_rate`

---

##### `--num-processes <COUNT>`
Number of simulated processes.

**Type**: Integer (u32)  
**Default**: `1`  
**Range**: `1` to `1000`  
**Example**: `--num-processes 4`

Each process has independent:
- Random number generator (derived from seed)
- Sequence numbers
- Clock

Messages from all processes are interleaved in the output.

**Config file equivalent**: `num_processes`

---

##### `--stdout` (Feature: `stdout`)
Enable stdout output mode.

**Type**: Boolean flag  
**Default**: `false`  
**Example**: `--stdout`

When enabled, messages are written to stdout as pretty-printed JSON instead of the Unix socket. Requires building with `--features stdout`.

**Config file equivalent**: `stdout`

---

##### `--json <PATH>` (Feature: `stdout`)
Write output to JSON file instead of socket.

**Type**: String (path)  
**Default**: None  
**Example**: `--json /tmp/output.json`

Collects all messages and writes them as a JSON array to the specified file. Requires building with `--features stdout`. The path is resolved relative to config file directory if specified in config file.

**Config file equivalent**: `json`

---

##### `-h, --help`
Print help information.

##### `-V, --version`
Print version information.

### Configuration File

#### Format

Configuration files use TOML format. The file path is specified with `--config`.

#### Example Configuration

```toml
# Basic settings
socket = "/tmp/telemetry.sock"
num_processes = 4
seed = 42

# Timing
duration_sec = 60
event_rate = 5.0
session_gap_mean_ms = 2000

# Session durations
duration_mean_ms = 120000
duration_stddev_ms = 60000

# Network simulation
interleave_prob = 0.1
mean_delay_ms = 50.0
drop_rate = 0.02
```

#### Configuration Options

##### `socket`
Unix domain socket path.

**Type**: String  
**Default**: `"/tmp/telemetry.sock"`  
**CLI equivalent**: `--socket`

**Example**:
```toml
socket = "/var/run/telemetry.sock"
```

---

##### `mean_delay_ms`
Mean network delay in milliseconds.

**Type**: Float  
**Default**: `0.0`  
**CLI equivalent**: `--mean-delay-ms`

**Example**:
```toml
mean_delay_ms = 50.0
```

---

##### `seed`
Random seed for deterministic output.

**Type**: Integer  
**Default**: None (random)  
**CLI equivalent**: `--seed`

**Example**:
```toml
seed = 42
```

---

##### `duration_sec`
Run duration in seconds.

**Type**: Integer  
**Default**: `0` (indefinite)  
**CLI equivalent**: `--duration-sec`

**Example**:
```toml
duration_sec = 300
```

Cannot be used with `min_sessions`.

---

##### `min_sessions`
Minimum sessions per process.

**Type**: Integer  
**Default**: `0` (no minimum)  
**CLI equivalent**: `--min-sessions`

**Example**:
```toml
min_sessions = 100
```

Cannot be used with `duration_sec`.

---

##### `session_gap_mean_ms`
Mean gap between sessions in milliseconds.

**Type**: Integer  
**Default**: `1000`  
**CLI equivalent**: `--session-gap-mean-ms`

**Example**:
```toml
session_gap_mean_ms = 5000
```

---

##### `event_rate`
Events per second per process.

**Type**: Float  
**Default**: `2.0`  
**CLI equivalent**: `--event-rate`

**Example**:
```toml
event_rate = 10.0
```

---

##### `duration_mean_ms`
Mean session duration in milliseconds.

**Type**: Integer  
**Default**: `60000`  
**CLI equivalent**: `--duration-mean-ms`

**Example**:
```toml
duration_mean_ms = 120000
```

---

##### `duration_stddev_ms`
Standard deviation of session duration.

**Type**: Integer  
**Default**: `120000`  
**CLI equivalent**: `--duration-stddev-ms`

**Example**:
```toml
duration_stddev_ms = 60000
```

---

##### `interleave_prob`
Message reordering probability.

**Type**: Float  
**Default**: `0.0`  
**CLI equivalent**: `--interleave-prob`

**Example**:
```toml
interleave_prob = 0.1
```

---

##### `start_time_ms`
Initial synthetic timestamp.

**Type**: Integer  
**Default**: `0`  
**CLI equivalent**: `--start-time-ms`

**Example**:
```toml
start_time_ms = 1640995200000
```

---

##### `drop_rate`
Message loss probability.

**Type**: Float  
**Default**: `0.0`  
**CLI equivalent**: `--drop-rate`

**Example**:
```toml
drop_rate = 0.05
```

---

##### `num_processes`
Number of simulated processes.

**Type**: Integer  
**Default**: `1`  
**CLI equivalent**: `--num-processes`

**Example**:
```toml
num_processes = 4
```

---

##### `stdout` (Feature: `stdout`)
Enable stdout output.

**Type**: Boolean  
**Default**: `false`  
**CLI equivalent**: `--stdout`

**Example**:
```toml
stdout = true
```

Requires building with `--features stdout`.

---

##### `json` (Feature: `stdout`)
JSON output file path.

**Type**: String  
**Default**: None  
**CLI equivalent**: `--json`

**Example**:
```toml
json = "/tmp/output.json"
```

Requires building with `--features stdout`. Path is resolved relative to config file directory.

### Configuration Precedence

When the same option is specified in multiple places, the following precedence applies:

1. **Command-line arguments** (highest priority)
2. **Configuration file**
3. **Built-in defaults** (lowest priority)

#### Example

Config file (`config.toml`):
```toml
num_processes = 2
event_rate = 5.0
```

Command line:
```bash
telemetry-sim --config config.toml --num-processes 4
```

Result:
- `num_processes` = 4 (from CLI, overrides config file)
- `event_rate` = 5.0 (from config file)
- Other options use defaults

### Mutual Exclusivity

#### Duration vs Session Count

`duration_sec` and `min_sessions` are mutually exclusive:

```toml
# Valid
duration_sec = 60

# Valid
min_sessions = 100

# Invalid - will error
duration_sec = 60
min_sessions = 100
```

### Validation

The simulator validates configuration on startup and will exit with an error if:

#### Socket Path
- Path exceeds 107 characters
  ```
  Error: Socket path too long (max 107 characters)
  ```

#### Numeric Ranges
- `interleave_prob` not in [0.0, 1.0]
- `drop_rate` not in [0.0, 1.0]
- `mean_delay_ms` < 0 or > 1,000,000
- `num_processes` < 1 or > 1000
- `session_gap_mean_ms` = 0
- `event_rate` ≤ 0
- `duration_mean_ms` = 0
- `duration_stddev_ms` = 0

#### Mutual Exclusivity
- Both `duration_sec` and `min_sessions` specified

### Path Resolution

Paths in configuration files are resolved as follows:

- **Absolute paths**: Used as-is
- **Relative paths**: Resolved relative to the config file's directory

**Example**:

Config file at `/etc/telemetry/config.toml`:
```toml
socket = "telemetry.sock"      # → /etc/telemetry/telemetry.sock
json = "../output.json"         # → /etc/output.json
```

Config file at `./config.toml` (current directory):
```toml
socket = "telemetry.sock"      # → ./telemetry.sock
```

CLI arguments are always resolved relative to current working directory.

### Complete Examples

#### Example 1: Basic Development Setup

```toml
# config/dev.toml
num_processes = 1
duration_sec = 30
event_rate = 2.0
```

Usage:
```bash
telemetry-sim --config config/dev.toml
```

#### Example 2: Load Testing

```toml
# config/load-test.toml
num_processes = 10
event_rate = 50.0
duration_sec = 300
session_gap_mean_ms = 100
interleave_prob = 0.05
mean_delay_ms = 10.0
```

Usage:
```bash
telemetry-sim --config config/load-test.toml --seed 42
```

#### Example 3: Deterministic Testing with Output

```toml
# config/test.toml
seed = 12345
num_processes = 2
min_sessions = 50
event_rate = 10.0
```

Usage (requires stdout feature):
```bash
cargo run --features stdout -- --config config/test.toml --json /tmp/test-output.json
```

#### Example 4: Network Simulation

```toml
# config/network-test.toml
num_processes = 4
duration_sec = 120
interleave_prob = 0.2
mean_delay_ms = 100.0
drop_rate = 0.05
```

Usage:
```bash
telemetry-sim --config config/network-test.toml
```

## Examples

### Basic Development

```bash
# Run for 30 seconds with default settings
cargo run -- --duration-sec 30
```

### Load Testing

```bash
# 10 processes, high event rate, 5 minute duration
cargo run -- --num-processes 10 --event-rate 50 --duration-sec 300 --seed 42
```

### Network Simulation

```bash
# Simulate network issues: 10% reordering, 5% loss, 50ms mean delay
cargo run -- --interleave-prob 0.1 --mean-delay-ms 50 --drop-rate 0.05
```

### Deterministic Testing

```bash
# Reproducible output with seed
cargo run --features stdout -- --seed 12345 --stdout --duration-sec 10
```

## Performance

The simulator is designed for high throughput:

- **Single process**: ~100k messages/second
- **Multiple processes**: Scales linearly with process count
- **Memory usage**: Minimal (stateless per message)
- **CPU usage**: Efficient random number generation and serialization

## License

See LICENSE file for details.

## Contributing

Contributions welcome! Please ensure:
- All tests pass (`cargo test`)
- Code is formatted (`cargo fmt`)
- No clippy warnings (`cargo clippy`)
- Documentation updated for user-facing changes

## See Also

- [Socket API Specification](docs/SOCKET_API.md) - Output protocol details
