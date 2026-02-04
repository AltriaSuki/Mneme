use crate::Executor;
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;

pub struct LocalExecutor;

impl LocalExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LocalExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Executor for LocalExecutor {
    async fn execute(&self, command: &str) -> Result<String> {
        // 使用 sh -c 来支持 shell 特性 (管道, 重定向等)
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .output()
            .await
            .context("Failed to execute command locally")?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            anyhow::bail!(
                "Command failed with status {}:\nStderr: {}",
                output.status,
                stderr
            );
        } else if !stderr.is_empty() {
             tracing::debug!("Command stderr (success): {}", stderr);
        }

        Ok(stdout.to_string())
    }
    
    fn name(&self) -> &str {
        "LocalExecutor"
    }
}
