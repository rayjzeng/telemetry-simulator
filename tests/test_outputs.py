import subprocess
import socket
import struct
import msgpack
import time
import os


def test_telemetry_sim_binary_exists():
    result = subprocess.run(["test", "-f", "/app/telemetry-sim"], capture_output=True)
    assert result.returncode == 0, "/app/telemetry-sim binary not found"


def test_burst_mode():
    socket_path = "/tmp/test-burst.sock"

    # Start simulator in background
    proc = subprocess.Popen(
        [
            "/app/telemetry-sim",
            "--socket",
            socket_path,
            "--rate",
            "0",
            "--count",
            "10",
            "--seed",
            "42",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    time.sleep(0.5)  # Wait for socket

    # Connect and read messages
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(socket_path)

    messages = []
    for _ in range(10):
        # Read length prefix
        len_bytes = sock.recv(4)
        assert len(len_bytes) == 4
        msg_len = struct.unpack(">I", len_bytes)[0]

        # Read message
        msg_data = sock.recv(msg_len)
        assert len(msg_data) == msg_len

        msg = msgpack.unpackb(msg_data, raw=False)
        messages.append(msg)

    sock.close()
    proc.terminate()
    proc.wait()

    # Validate
    assert len(messages) == 10
    for msg in messages:
        assert "type" in msg
        assert msg["type"] in ["event", "session"]
        assert "timestamp_ns" in msg
        assert msg["version"] == 1
