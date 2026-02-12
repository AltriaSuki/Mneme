use crate::config::{CapabilityTier, SafetyConfig};
use std::fmt;
use std::path::{Path, PathBuf};

// ============================================================================
// Error type
// ============================================================================

#[derive(Debug, Clone)]
pub struct SafetyDenied {
    pub reason: String,
    pub tier: CapabilityTier,
}

impl fmt::Display for SafetyDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Safety denied ({:?}): {}", self.tier, self.reason)
    }
}

impl std::error::Error for SafetyDenied {}

// ============================================================================
// CapabilityGuard
// ============================================================================

pub struct CapabilityGuard {
    config: SafetyConfig,
}

impl CapabilityGuard {
    pub fn new(config: SafetyConfig) -> Self {
        Self { config }
    }

    pub fn tier(&self) -> &CapabilityTier {
        &self.config.tier
    }

    /// Check if a shell command is allowed under the current tier.
    pub fn check_command(&self, command: &str) -> Result<(), SafetyDenied> {
        // Blocked commands always denied regardless of tier
        for blocked in &self.config.blocked_commands {
            if command.contains(blocked.as_str()) {
                return Err(SafetyDenied {
                    reason: format!("Command contains blocked pattern: '{}'", blocked),
                    tier: self.config.tier.clone(),
                });
            }
        }

        match self.config.tier {
            CapabilityTier::Full => Ok(()),
            CapabilityTier::Restricted => {
                // Restricted: allow most commands, block destructive ones
                if is_destructive_command(command) {
                    Err(SafetyDenied {
                        reason: format!(
                            "Destructive command not allowed in Restricted tier: '{}'",
                            command
                        ),
                        tier: self.config.tier.clone(),
                    })
                } else {
                    Ok(())
                }
            }
            CapabilityTier::ReadOnly => {
                // ReadOnly: only allow read-only commands
                if is_read_only_command(command) {
                    Ok(())
                } else {
                    Err(SafetyDenied {
                        reason: format!(
                            "Only read-only commands allowed in ReadOnly tier: '{}'",
                            command
                        ),
                        tier: self.config.tier.clone(),
                    })
                }
            }
        }
    }

    /// Check if a file path is allowed for write access.
    pub fn check_path(&self, path: &Path) -> Result<(), SafetyDenied> {
        match self.config.tier {
            CapabilityTier::Full => Ok(()),
            CapabilityTier::ReadOnly => Err(SafetyDenied {
                reason: "Write access denied in ReadOnly tier".to_string(),
                tier: self.config.tier.clone(),
            }),
            CapabilityTier::Restricted => {
                if self.config.allowed_paths.is_empty() {
                    // No whitelist configured — allow current directory
                    return Ok(());
                }
                for allowed in &self.config.allowed_paths {
                    // Try both raw and canonical comparisons to handle symlinks (e.g. /tmp → /private/tmp on macOS)
                    if path.starts_with(allowed) {
                        return Ok(());
                    }
                    let canonical_path = canonicalize_best_effort(path);
                    let canonical_allowed = canonicalize_best_effort(allowed);
                    if canonical_path.starts_with(&canonical_allowed) {
                        return Ok(());
                    }
                }
                Err(SafetyDenied {
                    reason: format!(
                        "Path '{}' is outside allowed paths in Restricted tier",
                        path.display()
                    ),
                    tier: self.config.tier.clone(),
                })
            }
        }
    }

    /// Check if a URL is allowed by the network whitelist.
    pub fn check_url(&self, url: &str) -> Result<(), SafetyDenied> {
        if self.config.network_whitelist.is_empty() {
            return Ok(()); // Empty whitelist = allow all
        }
        let host = extract_host(url);
        for allowed in &self.config.network_whitelist {
            if host == *allowed || host.ends_with(&format!(".{}", allowed)) {
                return Ok(());
            }
        }
        Err(SafetyDenied {
            reason: format!("Host '{}' not in network whitelist", host),
            tier: self.config.tier.clone(),
        })
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Best-effort path canonicalization (falls back to the original if it doesn't exist yet).
fn canonicalize_best_effort(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Extract the host portion from a URL string.
fn extract_host(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_string()
}

/// Heuristic: is this a read-only shell command?
fn is_read_only_command(command: &str) -> bool {
    let first_token = command.split_whitespace().next().unwrap_or("");
    let read_only_commands = [
        "ls",
        "cat",
        "head",
        "tail",
        "less",
        "more",
        "find",
        "grep",
        "wc",
        "file",
        "stat",
        "du",
        "df",
        "pwd",
        "echo",
        "date",
        "whoami",
        "hostname",
        "uname",
        "env",
        "printenv",
        "which",
        "git status",
        "git log",
        "git diff",
        "git show",
        "git branch",
        "curl",
        "wget", // GET by default
        "ps",
        "top",
        "htop",
        "free",
        "uptime",
    ];

    // Check multi-word commands first (e.g. "git status")
    for cmd in &read_only_commands {
        if cmd.contains(' ') && command.trim().starts_with(cmd) {
            return true;
        }
    }

    read_only_commands.contains(&first_token)
}

/// Heuristic: is this a destructive shell command?
fn is_destructive_command(command: &str) -> bool {
    let first_token = command.split_whitespace().next().unwrap_or("");
    let destructive_prefixes = [
        "rm", "rmdir", "mkfs", "dd", "fdisk", "parted", "shutdown", "reboot", "halt", "poweroff",
        "kill", "killall", "pkill",
    ];

    // "sudo" escalation
    if first_token == "sudo" {
        return true;
    }

    destructive_prefixes.contains(&first_token)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SafetyConfig;

    fn read_only_guard() -> CapabilityGuard {
        CapabilityGuard::new(SafetyConfig {
            tier: CapabilityTier::ReadOnly,
            allowed_paths: vec![],
            blocked_commands: vec!["rm -rf /".to_string()],
            network_whitelist: vec![],
            require_confirmation: false,
        })
    }

    fn restricted_guard() -> CapabilityGuard {
        CapabilityGuard::new(SafetyConfig {
            tier: CapabilityTier::Restricted,
            allowed_paths: vec![PathBuf::from("/tmp")],
            blocked_commands: vec!["rm -rf /".to_string()],
            network_whitelist: vec!["api.example.com".to_string()],
            require_confirmation: false,
        })
    }

    fn full_guard() -> CapabilityGuard {
        CapabilityGuard::new(SafetyConfig {
            tier: CapabilityTier::Full,
            allowed_paths: vec![],
            blocked_commands: vec!["rm -rf /".to_string()],
            network_whitelist: vec![],
            require_confirmation: false,
        })
    }

    // --- ReadOnly tier ---

    #[test]
    fn test_readonly_allows_ls() {
        let guard = read_only_guard();
        assert!(guard.check_command("ls -la").is_ok());
    }

    #[test]
    fn test_readonly_allows_git_status() {
        let guard = read_only_guard();
        assert!(guard.check_command("git status").is_ok());
    }

    #[test]
    fn test_readonly_denies_rm() {
        let guard = read_only_guard();
        assert!(guard.check_command("rm file.txt").is_err());
    }

    #[test]
    fn test_readonly_denies_write_path() {
        let guard = read_only_guard();
        assert!(guard.check_path(Path::new("/tmp/test.txt")).is_err());
    }

    // --- Restricted tier ---

    #[test]
    fn test_restricted_allows_ls() {
        let guard = restricted_guard();
        assert!(guard.check_command("ls -la").is_ok());
    }

    #[test]
    fn test_restricted_allows_echo() {
        let guard = restricted_guard();
        assert!(guard.check_command("echo hello").is_ok());
    }

    #[test]
    fn test_restricted_denies_rm() {
        let guard = restricted_guard();
        assert!(guard.check_command("rm file.txt").is_err());
    }

    #[test]
    fn test_restricted_denies_sudo() {
        let guard = restricted_guard();
        assert!(guard.check_command("sudo apt install foo").is_err());
    }

    #[test]
    fn test_restricted_allows_path_in_whitelist() {
        let guard = restricted_guard();
        assert!(guard.check_path(Path::new("/tmp/subdir/file.txt")).is_ok());
    }

    #[test]
    fn test_restricted_denies_path_outside_whitelist() {
        let guard = restricted_guard();
        assert!(guard.check_path(Path::new("/etc/passwd")).is_err());
    }

    #[test]
    fn test_restricted_url_whitelist() {
        let guard = restricted_guard();
        assert!(guard.check_url("https://api.example.com/v1/data").is_ok());
        assert!(guard.check_url("https://evil.com/steal").is_err());
    }

    // --- Full tier ---

    #[test]
    fn test_full_allows_rm() {
        let guard = full_guard();
        assert!(guard.check_command("rm file.txt").is_ok());
    }

    #[test]
    fn test_full_allows_any_path() {
        let guard = full_guard();
        assert!(guard.check_path(Path::new("/etc/passwd")).is_ok());
    }

    // --- Blocked commands always denied ---

    #[test]
    fn test_blocked_command_denied_even_in_full() {
        let guard = full_guard();
        assert!(guard.check_command("rm -rf /").is_err());
    }

    // --- URL helpers ---

    #[test]
    fn test_extract_host() {
        assert_eq!(
            extract_host("https://api.example.com/v1"),
            "api.example.com"
        );
        assert_eq!(extract_host("http://localhost:8080/path"), "localhost");
    }
}
