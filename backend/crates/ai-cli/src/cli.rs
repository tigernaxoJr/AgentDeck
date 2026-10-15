use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::engine::{
    AiEngine, AiError, AiInvocation, AiRawResult, Capabilities, TokenUsage,
};

pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<String>,
}

#[derive(Debug, Clone)]
pub enum StreamItem {
    TextDelta(String),
    FinalText(String),
    Usage(TokenUsage),
    Ignore,
}

pub trait CliAdapter: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn default_program(&self) -> String;
    fn capabilities(&self) -> Capabilities;
    fn version_args(&self) -> Vec<String> {
        vec!["--version".into()]
    }
    fn build_command(&self, program: &str, inv: &AiInvocation, run_dir: &Path) -> CommandSpec;
    fn parse_line(&self, line: &str) -> StreamItem;
}

pub fn which(program: &str) -> bool {
    let check_cmd = if cfg!(windows) { "where.exe" } else { "which" };
    std::process::Command::new(check_cmd)
        .arg(program)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub struct CliEngine<A: CliAdapter> {
    pub adapter: A,
    pub capabilities: Capabilities,
    pub program_override: Option<String>,
    pub model_override: Option<String>,
    pub base_runs_dir: PathBuf,
}

impl<A: CliAdapter> CliEngine<A> {
    pub fn new(adapter: A, base_runs_dir: PathBuf) -> Self {
        let capabilities = adapter.capabilities();
        Self {
            adapter,
            capabilities,
            program_override: None,
            model_override: None,
            base_runs_dir,
        }
    }

    pub fn with_overrides(
        adapter: A,
        program_override: Option<String>,
        model_override: Option<String>,
        base_runs_dir: PathBuf,
    ) -> Self {
        let capabilities = adapter.capabilities();
        Self {
            adapter,
            capabilities,
            program_override,
            model_override,
            base_runs_dir,
        }
    }

    fn resolved_program(&self) -> String {
        self.program_override
            .clone()
            .unwrap_or_else(|| self.adapter.default_program())
    }
}

#[async_trait::async_trait]
impl<A: CliAdapter> AiEngine for CliEngine<A> {
    fn id(&self) -> &str {
        self.adapter.id()
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn model(&self) -> Option<String> {
        self.model_override.clone()
    }

    async fn health_check(&self) -> Result<String, AiError> {
        let program = self.resolved_program();
        let args = self.adapter.version_args();
        let mut cmd = Command::new(&program);
        cmd.args(&args).stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AiError::NotInstalled(format!("{program} not found"))
            } else {
                AiError::Spawn(format!("Failed to run {program} --version: {e}"))
            }
        })?;

        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(s)
        } else {
            let err = String::from_utf8_lossy(&output.stderr).to_string();
            Err(AiError::NonZeroExit {
                code: output.status.code(),
                stderr_tail: err,
            })
        }
    }

    async fn run(&self, inv: AiInvocation) -> Result<AiRawResult, AiError> {
        let start = Instant::now();
        let run_dir = self.base_runs_dir.join(&inv.call_id);
        tokio::fs::create_dir_all(&run_dir)
            .await
            .map_err(|e| AiError::Spawn(format!("create run_dir: {e}")))?;
        tokio::fs::create_dir_all(run_dir.join("out"))
            .await
            .map_err(|e| AiError::Spawn(format!("create run_dir/out: {e}")))?;

        for file in &inv.files {
            let target = run_dir.join(&file.name);
            if let Some(parent) = target.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            tokio::fs::write(&target, &file.bytes)
                .await
                .map_err(|e| AiError::Spawn(format!("write run file {}: {e}", file.name)))?;
        }

        let program = self.resolved_program();
        let mut invocation = inv;
        if invocation.model.is_none() && self.model_override.is_some() {
            invocation.model = self.model_override.clone();
        }

        let cmd_spec = self.adapter.build_command(&program, &invocation, &run_dir);
        let mut cmd = Command::new(&cmd_spec.program);
        cmd.current_dir(&run_dir)
            .args(&cmd_spec.args)
            .envs(cmd_spec.env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if cmd_spec.stdin.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }

        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AiError::NotInstalled(cmd_spec.program.clone())
            } else {
                AiError::Spawn(e.to_string())
            }
        })?;

        if let Some(stdin_content) = cmd_spec.stdin {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(stdin_content.as_bytes()).await;
                let _ = stdin.flush().await;
            }
        }

        let stdout = child.stdout.take().ok_or_else(|| AiError::Spawn("take stdout".into()))?;
        let stderr = child.stderr.take().ok_or_else(|| AiError::Spawn("take stderr".into()))?;

        let timeout_fut = tokio::time::sleep(invocation.timeout);
        tokio::pin!(timeout_fut);

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        let mut accumulated_text = String::new();
        let mut final_text: Option<String> = None;
        let mut usage: Option<TokenUsage> = None;
        let mut stderr_tail = String::new();

        let mut child_exited = false;
        let mut exit_status = None;

        loop {
            tokio::select! {
                _ = &mut timeout_fut => {
                    let _ = child.kill().await;
                    return Err(AiError::Timeout);
                }
                res = stdout_reader.next_line() => {
                    match res {
                        Ok(Some(line)) => {
                            match self.adapter.parse_line(&line) {
                                StreamItem::TextDelta(d) => accumulated_text.push_str(&d),
                                StreamItem::FinalText(t) => final_text = Some(t),
                                StreamItem::Usage(u) => usage = Some(u),
                                StreamItem::Ignore => {}
                            }
                        }
                        Ok(None) => {
                            // stdout EOF
                            if child_exited { break; }
                        }
                        Err(_) => {}
                    }
                }
                res = stderr_reader.next_line() => {
                    match res {
                        Ok(Some(line)) => {
                            stderr_tail.push_str(&line);
                            stderr_tail.push('\n');
                            if stderr_tail.len() > 4096 {
                                let offset = stderr_tail.len() - 4096;
                                stderr_tail = stderr_tail[offset..].to_string();
                            }
                        }
                        Ok(None) => {
                            if child_exited { break; }
                        }
                        Err(_) => {}
                    }
                }
                status_res = child.wait(), if !child_exited => {
                    child_exited = true;
                    exit_status = status_res.ok();
                }
            }
        }

        // Check expect_output_file
        let mut output_file_content = None;
        if let Some(ref expect_rel) = invocation.expect_output_file {
            let out_path = run_dir.join(expect_rel);
            if out_path.exists() {
                if let Ok(content) = tokio::fs::read_to_string(&out_path).await {
                    output_file_content = Some(content);
                }
            }
        }

        let effective_final_text = final_text.or_else(|| {
            let trimmed = accumulated_text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        if output_file_content.is_none() && effective_final_text.is_none() {
            if let Some(status) = exit_status {
                if !status.success() {
                    return Err(AiError::NonZeroExit {
                        code: status.code(),
                        stderr_tail,
                    });
                }
            }
            return Err(AiError::EmptyOutput);
        }

        Ok(AiRawResult {
            backend_id: self.adapter.id().to_string(),
            output_file: output_file_content,
            final_text: effective_final_text,
            usage,
            duration: start.elapsed(),
            stderr_tail,
        })
    }
}
