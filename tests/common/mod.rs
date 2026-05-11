use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub struct TestSimulator {
    pub child: Child,
    pub socket_path: String,
}

impl TestSimulator {
    pub fn start(args: &[&str]) -> Self {
        let socket_path = format!("/tmp/test-{}.sock", uuid::Uuid::new_v4());

        let mut cmd_args = vec!["--socket", &socket_path];
        cmd_args.extend_from_slice(args);

        let child = Command::new("cargo")
            .args(&["run", "--"])
            .args(&cmd_args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to start simulator");

        // Wait for socket to be created (poll for 5 seconds)
        let start = std::time::Instant::now();
        while !std::path::Path::new(&socket_path).exists() {
            if start.elapsed() > Duration::from_secs(5) {
                panic!("Simulator failed to start");
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // Give it a moment to start listening
        std::thread::sleep(Duration::from_millis(100));

        Self { child, socket_path }
    }
}

impl Drop for TestSimulator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = std::fs::remove_file(&self.socket_path);
    }
}
