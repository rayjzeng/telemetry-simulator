# Telemetry Simulator Socket API Specification

## Overview

The telemetry simulator outputs messages over a Unix domain socket using a length-prefixed MessagePack framing protocol. Each message is serialized as MessagePack and preceded by a 4-byte length header.

## Transport

- **Protocol**: Unix Domain Socket (SOCK_STREAM)
- **Default Path**: `/tmp/telemetry.sock`
- **Byte Order**: Big-endian (network byte order) for length prefix

### Alternative Output Modes

When built with the `stdout` feature, the simulator supports alternative output modes:

- **JSON file output**: Use `--json <path>` to write all messages to a JSON file instead of the socket
- **Stdout mode**: Use `--stdout` to write human-readable JSON to stdout (one message per line)

These modes are useful for testing and debugging without requiring a socket client.

## Message Framing

Each message on the socket is framed as follows:

```
+----------------+-------------------+
| Length (4 bytes) | MessagePack Data |
|   (big-endian)   |   (variable)     |
+----------------+-------------------+
```

### Length Prefix

- **Size**: 4 bytes (uint32)
- **Encoding**: Big-endian (most significant byte first)
- **Value**: Length of the MessagePack payload in bytes (does not include the 4-byte header)
- **Max Message Size**: 4,294,967,295 bytes (uint32 max)

### Reading Messages

To read messages from the socket:

1. Read exactly 4 bytes from the socket
2. Decode as big-endian uint32 to get payload length `N`
3. Read exactly `N` bytes from the socket
4. Deserialize the `N` bytes as MessagePack to get a `Message` object
5. Repeat for next message

## Message Format

Messages are encoded as MessagePack objects with the following structure:

### Message Types

Messages are discriminated by the `type` field, which can be either `"event"` or `"session"`.

### Event Message

Sent when a user interaction event occurs. Events are independent messages and are not explicitly associated with sessions in the raw stream. Session association (based on timestamps) is typically done by downstream processors.

```json
{
  "type": "event",
  "event_id": "550e8400-e29b-41d4-a716-446655440000",
  "event_name": "click",
  "timestamp_ns": 1000000,
  "version": 1,
  "process_id": 0,
  "sequence_number": 42
}
```

#### Fields

| Field | Type | Description |
|-------|------|-------------|
| `type` | string | Always `"event"` |
| `event_id` | string | Unique identifier for this event (UUID v4 format) |
| `event_name` | string | Name of the event (e.g., `"click"`, `"view"`, `"purchase"`, `"scroll"`, `"hover"`) |
| `timestamp_ns` | uint64 | Monotonic timestamp in nanoseconds since simulator start |
| `version` | uint8 | Schema version (currently `1`) |
| `process_id` | uint32 | Identifier for the process that generated this event (0 to num_processes-1) |
| `sequence_number` | uint64 | Per-process monotonically increasing counter. Gaps indicate dropped messages. |

### Session Message

Sent when a session starts or ends.

```json
{
  "type": "session",
  "session_id": "550e8400-e29b-41d4-a716-446655440001",
  "session_type": "browsing",
  "timestamp_ns": 2000000,
  "is_start": true,
  "version": 1,
  "process_id": 0,
  "sequence_number": 43
}
```

#### Fields

| Field | Type | Description |
|-------|------|-------------|
| `type` | string | Always `"session"` |
| `session_id` | string | Unique identifier for this session (UUID v4 format) |
| `session_type` | string | Type of session (e.g., `"browsing"`, `"checkout"`, `"search"`). For session end events (`is_start=false`), this field contains the session type from the start event. |
| `timestamp_ns` | uint64 | Monotonic timestamp in nanoseconds since simulator start |
| `is_start` | boolean | `true` for session start, `false` for session end |
| `version` | uint8 | Schema version (currently `1`) |
| `process_id` | uint32 | Identifier for the process that generated this session event (0 to num_processes-1) |
| `sequence_number` | uint64 | Per-process monotonically increasing counter. Gaps indicate dropped messages. |

**Note**: Session start and end events share the same `session_id`. The `session_type` field is populated for both start and end events.

## Timestamp Semantics

### Clock

Timestamps are monotonically increasing values in nanoseconds. By default, the clock starts at 0 when the simulator begins. Use `--start-time-ms` to specify a Unix timestamp (in milliseconds) as the starting point.

Each simulated process maintains an independent clock that advances based on the event rate and session timing parameters.

### Event Generation Timing

- **Event intervals**: Generated using an exponential distribution with rate determined by `--event-rate` (events per second)
- **Session gaps**: Time between sessions follows an exponential distribution with mean specified by `--session-gap-mean-ms`
- **Session durations**: Follow a log-normal distribution with mean `--duration-mean-ms` and standard deviation `--duration-stddev-ms`

### Ordering Guarantees

1. **Per-process monotonic**: Timestamps are strictly increasing within each process (`process_id`)
2. **Session boundaries**: Session start timestamp < session end timestamp for the same session_id
3. **Delivery ordering**: Messages may be delivered out of timestamp order when `--mean-delay-ms > 0` and `--interleave-prob > 0`
4. **Determinism**: With a fixed `--seed`, the sequence is deterministic
5. **Cross-process ordering**: No guarantees about ordering between different processes



## Error Handling

### Connection Behavior

- The simulator binds to the socket and waits up to 30 seconds for a client to connect
- If no client connects within 30 seconds, the simulator exits with code 3
- If the socket file already exists, it is removed before binding
- Only one client connection is accepted; the simulator serves that client until completion

### Connection Errors

- If the socket file doesn't exist or connection fails, check that the simulator is running and the path is correct
- Default socket path is `/tmp/telemetry.sock`, configurable via `--socket` flag
- Socket path must be 107 characters or less (Unix domain socket limit)

### Partial Reads

The length prefix ensures message boundaries. If a read returns fewer bytes than expected:
- For the 4-byte header: Wait for more data or treat as connection close
- For the payload: Continue reading until `msg_len` bytes are received

### Deserialization Errors

If deserialization fails, verify the payload length matches the length prefix and that the full payload was received before deserializing.

### Shutdown and Exit Codes

The simulator may exit with the following codes:

| Code | Meaning |
|------|---------|
| 0 | Normal completion (duration reached, session count reached, or client disconnected) |
| 1 | Configuration error or invalid arguments |
| 3 | Accept timeout (no client connected within 30 seconds) |
| 4 | Write error (client disconnected or socket error) |

On shutdown (Ctrl+C or completion):
1. The simulator enters "draining" mode
2. All pending delayed messages are delivered
3. The socket is closed
4. Summary statistics are printed to stderr

## Multi-Process Simulation

When `--num-processes` > 1, the simulator generates messages from multiple independent processes:

### Process Isolation

- Each process has its own:
  - Independent clock (timestamps are per-process)
  - Sequence number counter (starts at 0 for each process)
  - Random number generator (seeded differently per process)
  - Session state (sessions don't cross process boundaries)

### Message Interleaving

Messages from different processes are interleaved in the output stream:
- Process selection is random but deterministic with a fixed seed
- Each process generates messages at its own rate
- The `process_id` field identifies which process generated each message

### Sequence Numbers

- Sequence numbers are **per-process**, not global
- Process 0: seq 0, 1, 2, 3...
- Process 1: seq 0, 1, 2, 3...
- Process 2: seq 0, 1, 2, 3...
- etc.

To detect data loss, track sequence numbers separately for each `process_id`.

## Versioning

- **Current Version**: 1
- **Version Field**: Each message includes a `version` field (uint8)
- **Compatibility**: Version increments indicate breaking changes to the schema
- **Migration**: Clients should check the version field and handle unknown versions gracefully

## Configuration Impact

Simulator flags affect the message stream:

| Flag | Default | Impact on Messages |
|------|---------|-------------------|
| `--seed` | none | Makes message sequence deterministic |
| `--duration-sec` | 0 | Run for N seconds (0 = run until min-sessions or indefinitely). Mutually exclusive with `--min-sessions`. |
| `--min-sessions` | 0 | Generate at least N sessions per process (0 = run until duration or indefinitely). Mutually exclusive with `--duration-sec`. |
| `--mean-delay-ms` | 0.0 | Adds delivery delays causing out-of-order timestamps when combined with `--interleave-prob` |
| `--interleave-prob` | 0.0 | Probability (0.0-1.0) that a message gets delayed by `--mean-delay-ms` |
| `--session-types` | browsing,checkout,search | Comma-separated list of session type names |
| `--event-names` | click,view,purchase,scroll,hover | Comma-separated list of event names |
| `--start-time-ms` | 0 | Sets initial timestamp value (nanoseconds = ms * 1,000,000) |
| `--drop-rate` | 0.0 | Probability (0.0-1.0) of message loss; creates gaps in sequence numbers |
| `--num-processes` | 1 | Number of simulated processes (1-1000); messages include process_id field |
| `--session-gap-mean-ms` | 1000 | Mean gap between sessions in milliseconds (exponential distribution) |
| `--event-rate` | 2.0 | Events per second per process (exponential distribution) |
| `--duration-mean-ms` | 60000 | Mean session duration in milliseconds (log-normal distribution) |
| `--duration-stddev-ms` | 120000 | Standard deviation of session duration in milliseconds |
| `--socket` | /tmp/telemetry.sock | Unix socket path to bind to |

### Execution Modes

The simulator supports three execution modes:

1. **Duration Mode** (`--duration-sec > 0`): Runs for the specified number of seconds, then drains pending messages and exits.

2. **Count Mode** (`--min-sessions > 0`): Generates at least the specified number of sessions per process, then drains pending messages and exits.

3. **Indefinite Mode** (both `--duration-sec` and `--min-sessions` are 0): Runs indefinitely until interrupted with Ctrl+C or the client disconnects.

**Note**: `--duration-sec` and `--min-sessions` are mutually exclusive.

## Data Loss Detection

When `--drop-rate` is configured, messages may be randomly dropped after generation but before delivery. Each process maintains an independent sequence counter to enable loss detection.

### How Drops Work

1. Messages are generated with sequential sequence numbers per process
2. Each message is independently evaluated for dropping based on `--drop-rate`
3. Dropped messages still occupy their sequence number (creating gaps)
4. The delivery timing is preserved even for dropped messages to maintain realistic timing

### Detecting Dropped Messages

To detect data loss:

1. Track the last seen `sequence_number` for each `process_id`
2. If current `sequence_number` > last + 1, messages were dropped
3. Gap size = `current - last - 1` indicates number of dropped messages

### Example

Process 0 with 10% drop rate:
```
Received: seq 0, 1, 2, 4, 5, 7, 8, 9, 10...
Dropped:  seq 3, 6 (detected by gaps)
```

### Session Pair Integrity

Session start and end messages are generated as a pair but may be dropped independently. This means:
- A session start may be received without its corresponding end (if end was dropped)
- A session end may be received without its start (if start was dropped)
- This simulates realistic data loss scenarios where partial session data is lost
- Applications should handle orphaned session starts and ends gracefully


