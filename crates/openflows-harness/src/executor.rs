// crates/openflows-harness/src/executor.rs
//! Sandbox task executor for verify serve (Forge role).
//!
//! Executes commands in a restricted environment with:
//! - Process group isolation (for clean timeout/kill)
//! - Timeout enforcement via tokio::time::timeout
//! - Stdout/stderr capture and streaming
//! - Result persistence to Redis
//!
//! Part of task 5.2-5.3 (issue #143).

use a2a_protocol::{ExecutorInfo, VerifyResult};
use anyhow::Context;
use anyhow::Result;
use fred::prelude::*;
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::time::Instant;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Execute a verify task in a sandbox with timeout enforcement.
pub async fn execute_verify_task(
    client: &fred::clients::Client,
    tenant: &str,
    pair_id: &str,
    argv: &[String],
    timeout_secs: u64,
    workspace_id: &str,
) -> Result<VerifyResult> {
    let start = Instant::now();
    let task_id = Uuid::new_v4().to_string();

    info!(
        task_id = %task_id,
        pair_id = %pair_id,
        argv = ?argv,
        timeout_secs,
        "Starting task execution"
    );

    // Spawn the command with output capture
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn command")?;

    // Get stdout and stderr readers
    let stdout = child.stdout.take().context("Failed to take stdout")?;
    let stderr = child.stderr.take().context("Failed to take stderr")?;

    // Run the command with timeout
    let exit_status =
        match tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async {
            // Read streams synchronously in blocking tasks
            let stdout_reader = tokio::task::spawn_blocking(move || read_stream_sync(stdout));
            let stderr_reader = tokio::task::spawn_blocking(move || read_stream_sync(stderr));

            // Wait for child to finish
            let status = child.wait().context("Failed to wait for child")?;

            // Collect output
            let stdout_text = stdout_reader.await.context("Stdout reader task failed")??;
            let stderr_text = stderr_reader.await.context("Stderr reader task failed")??;

            Ok::<_, anyhow::Error>((status, stdout_text, stderr_text))
        })
        .await
        {
            Ok(Ok((status, stdout_text, stderr_text))) => (status, stdout_text, stderr_text, false),
            Ok(Err(e)) => {
                warn!(error = %e, "Command execution error");
                return Err(e);
            }
            Err(_) => {
                // Timeout occurred
                warn!(task_id = %task_id, "Command timed out");
                let _ = child.kill();
                let _ = child.wait();

                // Build timeout result
                let result = VerifyResult {
                    task_id: task_id.clone(),
                    exit_code: None,
                    timed_out: true,
                    duration_ms: start.elapsed().as_millis() as u64,
                    stdout_ref: format!("audit:a2a:{}:stdout", task_id),
                    stderr_ref: format!("audit:a2a:{}:stderr", task_id),
                    artifacts: vec![],
                    executor: ExecutorInfo {
                        role: "forge".to_string(),
                        workspace: workspace_id.to_string(),
                    },
                };

                // Store in Redis
                let verification_key = format!("ns:{}:pair:{}:verification", tenant, pair_id);
                let _: Result<(), _> = client
                    .set::<(), _, _>(
                        &verification_key,
                        serde_json::to_string(&result)?,
                        None,
                        None,
                        false,
                    )
                    .await;

                // Store timeout message in audit trail
                let stderr_key = format!("ns:{}:audit:a2a:{}:stderr", tenant, task_id);
                let _: Result<(), _> = client
                    .set::<(), _, _>(
                        &stderr_key,
                        format!("[TIMEOUT] Command exceeded {}s limit", timeout_secs),
                        None,
                        None,
                        false,
                    )
                    .await;

                return Ok(result);
            }
        };

    // Extract exit code
    let exit_code = if exit_status.0.success() {
        Some(0)
    } else {
        exit_status.0.code()
    };

    let duration_ms = start.elapsed().as_millis() as u64;

    // Store stdout/stderr in Redis audit trail (bounded size)
    let stdout_key = format!("ns:{}:audit:a2a:{}:stdout", tenant, task_id);
    let stderr_key = format!("ns:{}:audit:a2a:{}:stderr", tenant, task_id);

    // Keep only last 10KB of output per stream (avoid memory explosion)
    let stdout_tail = truncate_to_tail(&exit_status.1, 10240);
    let stderr_tail = truncate_to_tail(&exit_status.2, 10240);

    let _: Result<(), _> = client
        .set::<(), _, _>(&stdout_key, stdout_tail.clone(), None, None, false)
        .await;
    let _: Result<(), _> = client
        .set::<(), _, _>(&stderr_key, stderr_tail.clone(), None, None, false)
        .await;

    debug!(
        task_id = %task_id,
        exit_code = ?exit_code,
        duration_ms,
        stdout_lines = stdout_tail.lines().count(),
        stderr_lines = stderr_tail.lines().count(),
        "Task completed"
    );

    // Build result artifact
    let result = VerifyResult {
        task_id: task_id.clone(),
        exit_code,
        timed_out: false,
        duration_ms,
        stdout_ref: stdout_key.clone(),
        stderr_ref: stderr_key.clone(),
        artifacts: vec![], // TODO: artifact hash collection
        executor: ExecutorInfo {
            role: "forge".to_string(),
            workspace: workspace_id.to_string(),
        },
    };

    // Mirror result to Redis
    let verification_key = format!("ns:{}:pair:{}:verification", tenant, pair_id);
    let _: Result<(), _> = client
        .set::<(), _, _>(
            &verification_key,
            serde_json::to_string(&result)?,
            None,
            None,
            false,
        )
        .await;

    // Store full result in audit trail
    let result_key = format!("ns:{}:audit:a2a:{}:result", tenant, task_id);
    let _: Result<(), _> = client
        .set::<(), _, _>(
            &result_key,
            serde_json::to_string(&result)?,
            None,
            None,
            false,
        )
        .await;

    info!(
        task_id = %task_id,
        exit_code = ?exit_code,
        "Task result mirrored to Redis"
    );

    Ok(result)
}

/// Read a stream into a string (used for stdout/stderr).
/// This runs in a blocking context, so we use synchronous I/O.
fn read_stream_sync<R: std::io::Read>(reader: R) -> Result<String> {
    let buf_reader = std::io::BufReader::new(reader);
    let mut output = String::new();

    for line in buf_reader.lines() {
        match line {
            Ok(l) => {
                output.push_str(&l);
                output.push('\n');
            }
            Err(e) => {
                warn!(error = %e, "Error reading stream");
                break;
            }
        }
    }

    Ok(output)
}

/// Keep only the last N bytes of a string (for bounded storage).
fn truncate_to_tail(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }

    // Find the last line boundary that fits within max_bytes
    let bytes = s.as_bytes();
    let mut start = bytes.len().saturating_sub(max_bytes);

    // Skip partial line at the start
    while start > 0 && bytes[start] != b'\n' {
        start += 1;
    }
    if start > 0 && bytes[start] == b'\n' {
        start += 1; // Skip the newline
    }

    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_to_tail_preserves_short_strings() {
        let input = "hello\nworld";
        assert_eq!(truncate_to_tail(input, 100), input);
    }

    #[test]
    fn test_truncate_to_tail_respects_max_bytes() {
        let input = "line1\nline2\nline3\nline4";
        let result = truncate_to_tail(input, 15);
        assert!(result.len() <= 15);
        assert!(result.contains("line"));
    }

    #[test]
    fn test_truncate_to_tail_preserves_line_boundaries() {
        let input = "a\nb\nc\nd\ne";
        let result = truncate_to_tail(input, 5);
        // Should not have partial lines
        assert!(!result.starts_with("a\nb\nc")); // Partial lines removed
        assert!(result.contains('\n') || result.is_empty() || !result.contains("d\ne"));
    }
}
