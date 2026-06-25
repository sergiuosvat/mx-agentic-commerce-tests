use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use std::sync::atomic::{AtomicU16, Ordering};

static NEXT_PORT: AtomicU16 = AtomicU16::new(8085);

/// Optional chain simulator startup settings.
#[derive(Clone, Debug, Default)]
pub struct ChainSimulatorOptions {
    /// When set, passed as `--round-duration` (milliseconds added per generated block).
    pub round_duration_ms: Option<u64>,
}

impl ChainSimulatorOptions {
    pub fn with_round_duration_ms(ms: u64) -> Self {
        Self {
            round_duration_ms: Some(ms),
        }
    }
}

#[derive(Default)]
pub struct ProcessManager {
    children: Vec<Child>,
    simulator_ports: Vec<u16>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind to port 0 and return the OS-assigned free port.
    pub fn find_free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .expect("Failed to bind ephemeral port")
            .local_addr()
            .expect("Failed to read ephemeral port")
            .port()
    }

    pub fn start_chain_simulator(&mut self) -> Result<u16, std::io::Error> {
        self.start_chain_simulator_with_options(ChainSimulatorOptions::default())
    }

    pub fn start_chain_simulator_with_options(
        &mut self,
        options: ChainSimulatorOptions,
    ) -> Result<u16, std::io::Error> {
        let mut port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
        while TcpStream::connect(format!("127.0.0.1:{port}")).is_ok() {
            println!("Port {port} occupied — trying next port.");
            port = NEXT_PORT.fetch_add(1, Ordering::SeqCst);
        }
        println!("Starting Chain Simulator on port {port}...");

        let mut cmd_name = "mx-chain-simulator-go".to_string();

        // Check common locations if not in PATH
        if Command::new(&cmd_name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_err()
        {
            // Check common locations if not in PATH
            if let Ok(cwd) = std::env::current_dir() {
                let local_bin = cwd.join("mx-chain-simulator-go");
                if local_bin.exists() {
                    cmd_name = local_bin.to_string_lossy().to_string();
                } else if let Ok(home) = std::env::var("HOME") {
                    let go_bin = PathBuf::from(home).join("go/bin/mx-chain-simulator-go");
                    if go_bin.exists() {
                        cmd_name = go_bin.to_string_lossy().to_string();
                    }
                }
            }
        }

        // Always start a fresh simulator owned by this ProcessManager.
        let mut cmd = Command::new(&cmd_name);
        cmd.arg("--server-port")
            .arg(port.to_string())
            .arg("--rounds-per-epoch")
            .arg("20")
            .arg("--skip-configs-download");

        if let Some(round_duration_ms) = options.round_duration_ms {
            cmd.arg("--round-duration")
                .arg(round_duration_ms.to_string());
            println!(
                "Chain Simulator round-duration: {}ms (~{}s per block)",
                round_duration_ms,
                round_duration_ms / 1000
            );
        }

        let child = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .inspect_err(|_| {
                println!("Failed to start chain simulator. Ensure 'mx-chain-simulator-go' is in PATH or ~/go/bin.");
            })?;

        self.children.push(child);
        self.simulator_ports.push(port);
        self.wait_for_port(port, 120);
        println!("Chain Simulator started.");
        Ok(port)
    }

    /// Start a Node.js service. Pass `port = 0` to bind an ephemeral port (returned).
    pub fn start_node_service(
        &mut self,
        name: &str,
        cwd: &str,
        script: &str,
        env: Vec<(&str, &str)>,
        port: u16,
    ) -> Result<u16, std::io::Error> {
        let owned: Vec<(String, String)> = env
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self.start_node_service_owned(name, cwd, script, owned, port)
    }

    pub fn start_node_service_owned(
        &mut self,
        name: &str,
        cwd: &str,
        script: &str,
        env: Vec<(String, String)>,
        port: u16,
    ) -> Result<u16, std::io::Error> {
        let port = if port == 0 {
            Self::find_free_port()
        } else {
            port
        };
        let port_str = port.to_string();

        println!("Starting {} on port {}...", name, port);
        let mut cmd = Command::new("node");
        cmd.current_dir(cwd)
            .arg(script)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let mut has_port = false;
        for (key, val) in env {
            if key == "PORT" {
                has_port = true;
                cmd.env("PORT", &port_str);
            } else {
                cmd.env(key, val);
            }
        }
        if !has_port {
            cmd.env("PORT", &port_str);
        }

        let child = cmd.spawn()?;
        self.children.push(child);
        self.wait_for_port(port, 40);
        println!("{} started on port {}.", name, port);
        Ok(port)
    }

    fn wait_for_port(&self, port: u16, retries: u32) {
        for _ in 0..retries {
            if TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        panic!(
            "Failed to connect to port {} after {} retries",
            port, retries
        );
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        for mut child in self.children.drain(..) {
            child.kill().ok();
            child.wait().ok();
        }

        // Removed global pkill that breaks parallel tests

        // Wait for allocated ports to be fully released (TIME_WAIT cleanup)
        for port in &self.simulator_ports {
            let port = *port;
            for i in 0..30 {
                if TcpStream::connect(format!("127.0.0.1:{}", port)).is_err() {
                    if i > 0 {
                        println!("Port {} released after {}ms.", port, i * 500);
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            if TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
                println!("Warning: port {} still in use after 15s cleanup wait.", port);
            }
        }
    }
}
