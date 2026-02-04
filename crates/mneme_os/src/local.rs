use crate::Executor;
use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::Command;

use std::time::Duration;

pub struct LocalExecutor {
    timeout: Duration,
}

impl Default for LocalExecutor {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
        }
    }
}

impl LocalExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }
}

#[async_trait]
impl Executor for LocalExecutor {
    async fn execute(&self, command: &str) -> Result<String> {
        // 使用 sh -c 来支持 shell 特性 (管道, 重定向等)
        let child = Command::new("sh")
            .arg("-c")
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn command locally")?;

        let output = match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
             Ok(res) => res.context("Failed to wait for command output")?,
             Err(_) => anyhow::bail!("Command execution timed out after {:?}", self.timeout),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            anyhow::bail!(
                "Command failed with status {}:\nStderr: {}\nStdout: {}",
                output.status,
                stderr,
                stdout
            );
        } else if !stderr.is_empty() {
             tracing::debug!("Command stderr (success): {}", stderr);
        }

        Ok(stdout.to_string())
    }

    fn name(&self) -> &str {
        "local"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_executor_success() {
        let executor = LocalExecutor::default();
        let res = executor.execute("echo hello").await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap().trim(), "hello");
    }

    #[tokio::test]
    async fn test_local_executor_timeout() {
        // Set a very short timeout
        let executor = LocalExecutor::with_timeout(Duration::from_millis(100));
        // Sleep for longer than timeout
        let res = executor.execute("sleep 1").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_local_executor_failure() {
        let executor = LocalExecutor::default();
        let res = executor.execute("exit 1").await;
        assert!(res.is_err());
    }
}
