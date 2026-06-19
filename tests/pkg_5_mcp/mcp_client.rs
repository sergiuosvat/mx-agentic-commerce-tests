use serde_json::json;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub struct McpClient {
    pub child: Child,
    reader: BufReader<ChildStdout>,
    stdin: ChildStdin,
}

impl McpClient {
    pub async fn new(chain_id: &str, pem_path: Option<&str>, gateway_url: &str) -> Self {
        Self::new_with_env(chain_id, pem_path, Vec::new(), gateway_url).await
    }

    pub async fn new_with_env(
        chain_id: &str,
        pem_path: Option<&str>,
        extra_env: Vec<(&str, &str)>,
        gateway_url: &str,
    ) -> Self {
        let mcp_path = "dist/index.js";
        let working_dir = "../multiversx-mcp-server";

        let mut cmd = Command::new("node");
        cmd.arg(mcp_path)
            .arg("mcp")
            .current_dir(working_dir)
            .env("MVX_API_URL", gateway_url)
            .env("MVX_CHAIN_ID", chain_id)
            .env("MVX_PK_HEX", "");

        if let Some(pem) = pem_path {
            cmd.env("MVX_WALLET_PEM", pem);
        }

        for (key, val) in &extra_env {
            cmd.env(key, val);
        }

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Failed to start MCP server");

        let mut client = McpClient {
            stdin: child.stdin.take().expect("Failed to open stdin"),
            reader: BufReader::new(child.stdout.take().expect("Failed to open stdout")),
            child,
        };

        client.initialize().await;
        client
    }

    async fn initialize(&mut self) {
        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test-client", "version": "1.0.0" }
            }
        });
        self.send(init_req).await;
        let resp = self.read_response().await;
        assert!(resp.contains("serverInfo"), "Failed to initialize MCP");

        let notif = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        self.send(notif).await;
    }

    pub async fn call_tool(&mut self, name: &str, args: serde_json::Value) -> serde_json::Value {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": args
            }
        });
        self.send(req).await;
        let line = self.read_response().await;
        serde_json::from_str(&line).expect("Invalid JSON")
    }

    async fn send(&mut self, val: serde_json::Value) {
        let s = val.to_string();
        self.stdin.write_all(s.as_bytes()).await.unwrap();
        self.stdin.write_all(b"\n").await.unwrap();
    }

    async fn read_response(&mut self) -> String {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = self
                .reader
                .read_line(&mut line)
                .await
                .expect("Failed to read");
            if bytes == 0 {
                panic!("EOF from MCP Server");
            }
            let trimmed = line.trim();
            if trimmed.starts_with("{") {
                return line;
            }
        }
    }
}
