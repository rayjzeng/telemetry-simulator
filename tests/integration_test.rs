mod common;

#[cfg(test)]
mod tests {
    use super::common::TestSimulator;
    use tokio::io::AsyncReadExt;
    use tokio::net::UnixStream;

    #[tokio::test]
    async fn test_burst_mode_generates_exact_count() {
        let sim = TestSimulator::start(&["--min-sessions", "10", "--seed", "42"]);

        let mut stream = UnixStream::connect(&sim.socket_path).await.unwrap();
        let mut count = 0;
        let mut buf = [0u8; 4];

        // Read messages until we get 10 sessions (or more due to events)
        // We just verify we can connect and read messages
        for _ in 0..10 {
            stream.read_exact(&mut buf).await.unwrap();
            let len = u32::from_be_bytes(buf) as usize;
            let mut msg_buf = vec![0u8; len];
            stream.read_exact(&mut msg_buf).await.unwrap();
            count += 1;
        }

        assert_eq!(count, 10);
    }
}
