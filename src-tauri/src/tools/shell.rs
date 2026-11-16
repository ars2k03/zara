use std::env;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::tools;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_OUTPUT_BYTES: usize = 12_000;

pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn combined(&self) -> String {
        let mut parts = Vec::new();
        if !self.stdout.trim().is_empty() {
            parts.push(self.stdout.trim().to_string());
        }
        if !self.stderr.trim().is_empty() {
            parts.push(format!("stderr: {}", self.stderr.trim()));
        }
        if parts.is_empty() {
            format!("Completed with exit code {}.", self.exit_code)
        } else {
            truncate_output(&parts.join("\n"), MAX_OUTPUT_BYTES)
        }
    }
}

pub fn command_exists(name: &str) -> bool {
    let path = Path::new(name);
    if path.components().count() > 1 {
        return path.is_file();
    }
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|dir| dir.join(name).is_file())
}

pub fn truncate_output(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n...[truncated]", &text[..end])
}

pub async fn run_program(
    program: &str,
    args: &[String],
    cwd: Option<&str>,
    timeout_secs: u64,
) -> AppResult<CommandOutput> {
    run_program_internal(program, args, cwd, None, timeout_secs).await
}

pub async fn run_program_with_stdin(
    program: &str,
    args: &[String],
    stdin_data: &str,
    timeout_secs: u64,
) -> AppResult<CommandOutput> {
    run_program_internal(program, args, None, Some(stdin_data.as_bytes()), timeout_secs).await
}

async fn run_program_internal(
    program: &str,
    args: &[String],
    cwd: Option<&str>,
    stdin_bytes: Option<&[u8]>,
    timeout_secs: u64,
) -> AppResult<CommandOutput> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(if stdin_bytes.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(dir) = cwd.filter(|value| !value.trim().is_empty()) {
        cmd.current_dir(dir);
    }

    let mut child = cmd
        .spawn()
        .map_err(|err| AppError::Message(format!("Failed to start {program}: {err}")))?;

    if let Some(bytes) = stdin_bytes
        && let Some(mut stdin) = child.stdin.take()
    {
        stdin.write_all(bytes)?;
    }

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_handle = thread::spawn(move || read_pipe(stdout_pipe));
    let stderr_handle = thread::spawn(move || read_pipe(stderr_pipe));

    let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Message(format!(
                "Command `{program}` timed out after {timeout_secs}s."
            )));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();

    Ok(CommandOutput {
        exit_code: status.code().unwrap_or(-1),
        stdout: truncate_output(&stdout, MAX_OUTPUT_BYTES),
        stderr: truncate_output(&stderr, MAX_OUTPUT_BYTES),
    })
}

fn read_pipe<R: Read>(pipe: Option<R>) -> String {
    let Some(mut reader) = pipe else {
        return String::new();
    };
    let mut buffer = Vec::new();
    let _ = reader.read_to_end(&mut buffer);
    String::from_utf8_lossy(&buffer).into_owned()
}

pub async fn run_bash(command: &str, cwd: Option<&str>, timeout_secs: u64) -> AppResult<CommandOutput> {
    let args = vec!["-c".to_string(), command.to_string()];
    run_program("bash", &args, cwd, timeout_secs).await
}

pub fn spawn_detached(program: &str, args: &[String]) -> AppResult<u32> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc_setsid();
                Ok(())
            });
        }
    }

    let child = cmd
        .spawn()
        .map_err(|err| AppError::Message(format!("Failed to launch {program}: {err}")))?;
    Ok(child.id())
}

#[cfg(unix)]
fn libc_setsid() {
    unsafe extern "C" {
        fn setsid() -> i32;
    }
    unsafe {
        setsid();
    }
}

pub fn is_destructive_shell(command: &str) -> Option<&'static str> {
    let normalized = command.to_lowercase();
    let checks = [
        ("rm -rf /", "Root filesystem deletion"),
        ("rm -fr /", "Root filesystem deletion"),
        ("rm -rf ~", "Home directory deletion"),
        ("mkfs", "Disk formatting"),
        ("dd if=", "Raw disk write"),
        (":(){:|:&};:", "Fork bomb"),
        ("shutdown", "System shutdown"),
        ("reboot", "System reboot"),
        ("poweroff", "System poweroff"),
    ];
    checks
        .into_iter()
        .find(|(pattern, _)| normalized.contains(pattern))
        .map(|(_, label)| label)
}

pub async fn execute_run_shell(app: &AppHandle, state: &AppState, args: Value) -> AppResult<String> {
    let command = tools::req_str(&args, "command")?;
    let cwd = tools::opt_str(&args, "cwd");
    let timeout = args
        .get("timeout_secs")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .min(300);

    if let Some(reason) = is_destructive_shell(&command) {
        tools::confirm_action(
            app,
            state,
            format!("ঝুঁকিপূর্ণ কমান্ড: {reason}"),
            format!("কমান্ড: {command}"),
        )
        .await?;
    }

    let output = run_bash(&command, cwd.as_deref(), timeout).await?;
    Ok(format!("exit_code: {}\n{}", output.exit_code, output.combined()))
}
