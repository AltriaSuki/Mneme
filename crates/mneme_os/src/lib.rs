pub mod local;
pub mod ssh;

use anyhow::Result;
use async_trait::async_trait;

/// Executor trait 定义了执行系统命令的能力
///
/// Implementors:
/// - `LocalExecutor`: 在本地直接执行
/// - `SshExecutor`: 通过 SSH 远程执行 (或连接 localhost)
#[async_trait]
pub trait Executor: Send + Sync {
    /// 执行命令并返回标准输出
    async fn execute(&self, command: &str) -> Result<String>;

    /// 获取 Executor 类型名称 (用于日志)
    fn name(&self) -> &str;
}
