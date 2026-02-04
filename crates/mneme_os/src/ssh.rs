use crate::Executor;
use anyhow::{Context, Result};
use async_trait::async_trait;
use russh::*;
use russh_keys::*;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

struct ClientHandler;

#[async_trait]
impl client::Handler for ClientHandler {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &key::PublicKey,
    ) -> Result<bool, Self::Error> {
        // 在生产环境中应该验证 key，但在 MVP 阶段我们允许所有 (类似 ssh -o StrictHostKeyChecking=no)
        // 尤其是针对 localhost
        Ok(true)
    }
}

pub struct SshExecutor {
    user: String,
    host: String,
    port: u16,
    key_path: PathBuf,
    timeout: Duration,
}

impl SshExecutor {
    pub fn new(host: &str, user: &str, key_path: PathBuf) -> Self {
        Self {
            host: host.to_string(),
            user: user.to_string(),
            port: 22, // Default SSH port
            key_path,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    // Getters for testing
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    async fn connect(&self) -> Result<client::Handle<ClientHandler>> {
        let config = client::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(10)),
            ..Default::default()
        };
        let config = Arc::new(config);
        let handler = ClientHandler;

        let addr_str = format!("{}:{}", self.host, self.port);
        // Use tokio::net::lookup_host for non-blocking DNS resolution
        let mut addrs = tokio::net::lookup_host(&addr_str)
            .await
            .context("Could not resolve address")?;
        let addr = addrs.next().ok_or_else(|| anyhow::anyhow!("Could not resolve address"))?;

        // russh::client::connect handles the TCP connection itself
        let mut session = client::connect(config, addr, handler)
            .await
            .context("Failed to start SSH session")?;

        // 加载私钥
        let key_pair = load_secret_key(&self.key_path, None)?;

        let auth_res = session
            .authenticate_publickey(&self.user, Arc::new(key_pair))
            .await?;

        if !auth_res {
            anyhow::bail!("SSH authentication failed");
        }

        Ok(session)
    }
}

#[async_trait]
impl Executor for SshExecutor {
    async fn execute(&self, command: &str) -> Result<String> {
        let session = self.connect().await?;
        let mut channel = session.channel_open_session().await?;

        channel.exec(true, command).await?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exec_future = async {
            // 简单的读取循环
            while let Some(msg) = channel.wait().await {
                match msg {
                    ChannelMsg::Data { ref data } => {
                        stdout.extend_from_slice(data);
                    }
                    ChannelMsg::ExtendedData { ref data, .. } => {
                        stderr.extend_from_slice(data);
                    }
                    ChannelMsg::ExitStatus { exit_status } => {
                        if exit_status != 0 {
                            // Collect stderr before bailing
                            let stderr_str = String::from_utf8_lossy(&stderr);
                            let stdout_str = String::from_utf8_lossy(&stdout);
                            anyhow::bail!(
                                "Command failed with exit code: {}\nStderr: {}\nStdout: {}",
                                exit_status,
                                stderr_str,
                                stdout_str
                            );
                        }
                    }
                    _ => {}
                }
            }
            Ok(())
        };

        match tokio::time::timeout(self.timeout, exec_future).await {
            Ok(result) => result?,
            Err(_) => anyhow::bail!("Command execution timed out after {:?}", self.timeout),
        }

        Ok(String::from_utf8(stdout).context("Output was not valid UTF-8")?)
    }

    fn name(&self) -> &str {
        "ssh"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_executor_construction() {
        let exec = SshExecutor::new("localhost", "user", PathBuf::from("/tmp/key"))
            .with_timeout(Duration::from_secs(10))
            .with_port(2222);

        assert_eq!(exec.name(), "ssh");
        // Use public getters instead of private fields
        assert_eq!(exec.port(), 2222);
        assert_eq!(exec.timeout(), Duration::from_secs(10));
    }
}
