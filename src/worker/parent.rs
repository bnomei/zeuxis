use std::{path::Path, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

use crate::mcp::errors::ServerError;

use super::contract::{WorkerRequest, WorkerSuccessPayload, parse_response_json};

const HARD_KILL_WAIT_FALLBACK: Duration = Duration::from_millis(2_000);

pub async fn run_worker_capture(
    executable: &Path,
    request: &WorkerRequest,
    timeout: Duration,
    kill_grace: Duration,
    max_stdout_bytes: u64,
) -> Result<WorkerSuccessPayload, ServerError> {
    let request_json = serde_json::to_vec(request).map_err(|err| {
        ServerError::storage_failed(format!("failed to encode worker request JSON: {err}"))
    })?;

    let mut child = Command::new(executable)
        .arg("__worker")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|err| {
            ServerError::storage_failed(format!(
                "failed to spawn capture worker {}: {err}",
                executable.display()
            ))
        })?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| ServerError::storage_failed("capture worker stdin pipe was unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ServerError::storage_failed("capture worker stdout pipe was unavailable"))?;

    let write_task = tokio::spawn(async move {
        stdin.write_all(&request_json).await?;
        stdin.flush().await?;
        stdin.shutdown().await
    });
    let read_task = tokio::spawn(read_stdout_limited(stdout, max_stdout_bytes));

    let timed_wait = tokio::time::timeout(timeout, child.wait()).await;
    let status = match timed_wait {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => {
            let _ = drain_join(write_task).await;
            let _ = drain_join(read_task).await;
            return Err(ServerError::storage_failed(format!(
                "capture worker wait failed: {err}"
            )));
        }
        Err(_) => {
            terminate_worker(&mut child, kill_grace).await?;
            let _ = drain_join(write_task).await;
            let _ = drain_join(read_task).await;
            return Err(ServerError::storage_failed(format!(
                "capture timed out after {}ms",
                timeout.as_millis()
            )));
        }
    };

    drain_join(write_task).await?.map_err(|err| {
        ServerError::storage_failed(format!("failed to write capture worker request: {err}"))
    })?;
    let stdout_bytes = drain_join(read_task).await??;
    let stdout_text = String::from_utf8(stdout_bytes).map_err(|err| {
        ServerError::storage_failed(format!("capture worker stdout was not valid UTF-8: {err}"))
    })?;
    let response = parse_response_json(&stdout_text).map_err(|error| {
        ServerError::storage_failed(format!(
            "capture worker response was invalid: {}",
            error.message
        ))
    })?;

    if response.request_id != request.request_id {
        return Err(ServerError::storage_failed(format!(
            "capture worker response request_id mismatch: expected {} got {}",
            request.request_id, response.request_id
        )));
    }

    if !status.success() {
        return Err(ServerError::storage_failed(format!(
            "capture worker exited with status {status}"
        )));
    }

    if response.ok {
        return response.result.ok_or_else(|| {
            ServerError::storage_failed("capture worker success response missing result payload")
        });
    }

    let Some(error) = response.error else {
        return Err(ServerError::storage_failed(
            "capture worker error response missing error payload",
        ));
    };
    Err(error.to_server_error())
}

async fn read_stdout_limited(
    mut stdout: tokio::process::ChildStdout,
    max_stdout_bytes: u64,
) -> Result<Vec<u8>, ServerError> {
    let max_stdout_bytes = usize::try_from(max_stdout_bytes).unwrap_or(usize::MAX);
    let mut output = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let bytes_read = stdout.read(&mut chunk).await.map_err(|err| {
            ServerError::storage_failed(format!("failed to read capture worker stdout: {err}"))
        })?;
        if bytes_read == 0 {
            break;
        }
        if output.len().saturating_add(bytes_read) > max_stdout_bytes {
            return Err(ServerError::storage_failed(format!(
                "capture worker stdout exceeded {} bytes",
                max_stdout_bytes
            )));
        }
        output.extend_from_slice(&chunk[..bytes_read]);
    }
    Ok(output)
}

async fn terminate_worker(
    child: &mut tokio::process::Child,
    kill_grace: Duration,
) -> Result<(), ServerError> {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            // SAFETY: libc::kill is called with a PID obtained from tokio::process::Child::id.
            let signal_result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if signal_result != 0 {
                let err = std::io::Error::last_os_error();
                tracing::warn!(
                    pid,
                    error = %err,
                    "capture worker SIGTERM failed; escalating to hard kill"
                );
            }
        }
    }

    match tokio::time::timeout(kill_grace, child.wait()).await {
        // Only a confirmed clean exit lets us skip the hard kill.
        Ok(Ok(_status)) => return Ok(()),
        // `child.wait()` itself errored: we cannot confirm the child exited, so
        // escalate to a hard kill rather than treat the I/O error as success.
        Ok(Err(err)) => {
            tracing::warn!(
                error = %err,
                "capture worker wait failed during grace period; escalating to hard kill"
            );
        }
        // Grace period elapsed; the child is still running.
        Err(_) => {}
    }

    child.kill().await.map_err(|err| {
        ServerError::storage_failed(format!("failed to hard-kill capture worker: {err}"))
    })?;

    tokio::time::timeout(HARD_KILL_WAIT_FALLBACK, child.wait())
        .await
        .map_err(|_| ServerError::storage_failed("capture worker did not exit after hard kill"))?
        .map_err(|err| {
            ServerError::storage_failed(format!(
                "capture worker wait after hard kill failed: {err}"
            ))
        })?;
    Ok(())
}

async fn drain_join<T>(handle: tokio::task::JoinHandle<T>) -> Result<T, ServerError> {
    handle
        .await
        .map_err(|err| ServerError::storage_failed(format!("worker task join failed: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::contract::{
        CaptureOperation, WORKER_CONTRACT_VERSION, WorkerOutputFormat, WorkerOutputOptions,
    };

    #[test]
    fn worker_parent_constants_are_sane() {
        assert!(HARD_KILL_WAIT_FALLBACK >= Duration::from_millis(100));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn worker_parent_read_stdout_limited_errors_when_output_is_too_large() {
        let mut child = Command::new("sh")
            .arg("-lc")
            .arg("printf '1234567890'")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("spawn shell");
        let stdout = child.stdout.take().expect("stdout");

        let error = read_stdout_limited(stdout, 5)
            .await
            .expect_err("should fail");
        assert_eq!(error.error_code(), "storage_failed");
    }

    #[test]
    fn worker_parent_request_is_constructable() {
        let request = WorkerRequest {
            v: WORKER_CONTRACT_VERSION,
            request_id: "req-1".to_owned(),
            operation: CaptureOperation::CaptureScreen { monitor_id: None },
            output: WorkerOutputOptions {
                format: WorkerOutputFormat::Png,
                jpeg_quality: 82,
                max_dimension: None,
            },
            artifact_path: "/tmp/zeuxis-parent-test.png".to_owned(),
        };
        assert!(request.validate().is_ok());
    }
}
