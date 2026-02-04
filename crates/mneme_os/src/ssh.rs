use crate::Executor;
use anyhow::{Context, Result};
use async_trait::async_trait;
use russh::*;
use russh_keys::*;
use std::sync::Arc;
use std::net::SocketAddr;

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
        let mut session = client::connect(config, addr, handler).await.context("Failed to start SSH session")?;

        // 加载私钥
        let key_pair = load_secret_key(&self.key_path, None)?;
        
        // authenticate_publickey requires mutable access if it modifies internal state, 
        // but let's check if the handle needs to be mut. 
        // The previous error said `session` didn't need to be mutable for `channel_open_session`, 
        // but authenticate might. Let's keep it immutable first as the warning suggested, 
        // or check if authenticate requires &mut. 
        // Actually, Handle refers to a channel to the background task, so almost all methods take &self.
        let auth_res = session.authenticate_publickey(&self.user, Arc::new(key_pair)).await?;
        
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
        // 简单的读取循环
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { ref data } => {
                    stdout.extend_from_slice(data);
                }
                ChannelMsg::ExitStatus { exit_status } => {
                    if exit_status != 0 {
                        // 如果需要收集 stderr，这里需要更复杂的逻辑，russh 将 stderr 发送为 ExtendedData
                        // 为简单起见，MVP 暂时只返回 exit code 非零错误
                        // 实际改进: 区分 stdout/stderr
                         anyhow::bail!("Command failed with exit code: {}", exit_status);
                    }
                }
                _ => {} 
            }
        }

        Ok(String::from_utf8(stdout).context("Output was not valid UTF-8")?)
    }

    fn name(&self) -> &str {
        "SshExecutor"
    }
}
