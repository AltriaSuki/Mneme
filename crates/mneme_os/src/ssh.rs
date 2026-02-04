use crate::Executor;
use anyhow::{Context, Result};
use async_trait::async_trait;
use russh::*;
use russh_keys::*;
use std::net::SocketAddr;
use std::sync::Arc;

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
    key_path: String,
}

impl SshExecutor {
    pub fn new(user: String, host: String, port: u16, key_path: String) -> Self {
        Self {
            user,
            host,
            port,
            key_path,
        }
    }

    async fn connect(&self) -> Result<client::Handle<ClientHandler>> {
        let config = client::Config {
            ..Default::default()
        };
        let config = Arc::new(config);
        let handler = ClientHandler;

        let addr: SocketAddr = format!("{}:{}", self.host, self.port)
            .parse()
            .context("Invalid address")?;

        // russh::client::connect handles the TCP connection itself
        let mut session = client::connect(config, addr, handler)
            .await
            .context("Failed to start SSH session")?;

        // 加载私钥
        let key_pair = load_secret_key(&self.key_path, None)?;

        // authenticate_publickey requires mutable access if it modifies internal state
        let auth_res = session
            .authenticate_publickey(&self.user, Arc::new(key_pair))
            .await?;

        if !auth_res {
            anyhow::bail!("Authentication failed");
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

        let timeout_duration = std::time::Duration::from_secs(30);
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

        match tokio::time::timeout(timeout_duration, exec_future).await {
            Ok(result) => result?,
            Err(_) => anyhow::bail!("Command execution timed out after 30 seconds"),
        }

        Ok(String::from_utf8(stdout).context("Output was not valid UTF-8")?)
    }

    fn name(&self) -> &str {
        "SshExecutor"
    }
}
