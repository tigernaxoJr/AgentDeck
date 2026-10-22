use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::{interval, timeout, MissedTickBehavior};
use uuid::Uuid;

use ai_cli::adapters::AgyAdapter;
use ai_cli::{AiInvocation, AiPurpose, CliAdapter};

use crate::models::protocol::{
    ArtifactData, ArtifactType, CreateTaskRequest, LogEntry, ServerEvent, StreamChannel, TaskState,
    TokenUsage,
};

const SUMMARY_REL_PATH: &str = ".antigravity_out/summary.json";

pub struct TaskRunner {
    pub task_id: Uuid,
    pub req: CreateTaskRequest,
    pub base_workspace: PathBuf,
    pub event_tx: broadcast::Sender<ServerEvent>,
}

impl TaskRunner {
    /// 執行 CLI 任務，傳入 abort_rx 以支援執行期強制中止
    pub async fn run(self, mut abort_rx: oneshot::Receiver<()>) -> TaskState {
        // 1. 驗證並解析安全的工作目錄 (防止 Path Traversal)
        let work_dir = match self.resolve_safe_workspace() {
            Ok(dir) => dir,
            Err(err_msg) => {
                self.emit_system_log(format!("Workspace 驗證失敗: {}", err_msg));
                self.emit_error("INVALID_WORKSPACE", &err_msg);
                return TaskState::Failed;
            }
        };

        // 2. 準備結構化輸出目錄與附加系統 Prompt
        let final_prompt = self.prepare_prompt_and_output_dir(&work_dir).await;

        // 3. 由 ai-cli 的 adapter 組出 CLI 指令（旗標與 Tutor 共用同一份定義）
        let adapter = AgyAdapter;
        let bin_name = adapter.default_program();
        let inv = AiInvocation {
            call_id: self.task_id.to_string(),
            purpose: AiPurpose::Console,
            prompt: final_prompt,
            files: Vec::new(),
            images: Vec::new(),
            expect_output_file: None,
            model: self
                .req
                .model
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty())
                .map(str::to_string),
            timeout: Duration::from_secs(self.req.timeout_secs),
            extra_args: self.req.extra_flags.clone(),
        };
        let spec = adapter.build_command(&bin_name, &inv, &work_dir);

        let mut cmd = Command::new(&spec.program);
        cmd.current_dir(&work_dir)
            .args(&spec.args)
            .envs(spec.env)
            .stdin(Stdio::null()) // 切斷標準輸入，防止 Zombie Process
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true); // 當 Task 被 Drop 時自動由 OS 終止子行程

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("無法啟動 CLI 行程 ({}): {}", bin_name, e);
                self.emit_system_log(msg.clone());
                self.emit_error("SPAWN_FAILED", &msg);
                return TaskState::Failed;
            }
        };

        self.emit_system_log(format!(
            "PID {} 已啟動，工作目錄: {:?}",
            child.id().unwrap_or(0),
            work_dir
        ));

        // 4. 啟動 stdout / stderr 非同步讀取與 50ms 批次聚合器
        let stdout = child.stdout.take().expect("Failed to open stdout");
        let stderr = child.stderr.take().expect("Failed to open stderr");

        let (log_tx, log_rx) = mpsc::channel::<(StreamChannel, String)>(2048);
        let batcher_handle = tokio::spawn(Self::run_log_batcher(
            self.task_id,
            log_rx,
            self.event_tx.clone(),
        ));

        // 捕獲 Token Usage
        let captured_usage = Arc::new(std::sync::Mutex::new(None::<TokenUsage>));
        let captured_usage_clone = captured_usage.clone();

        // 分別啟動 stdout 與 stderr 的讀取 Task
        let tx_out = log_tx.clone();
        let stdout_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                // 解析 stream-json NDJSON
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                    // 1. 捕捉 usage 資訊
                    let usage_val = val.pointer("/result/usage")
                        .or_else(|| val.pointer("/step_update/usage"))
                        .or_else(|| val.get("usage"));
                    if let Some(u) = usage_val
                        && let Ok(parsed) = serde_json::from_value::<TokenUsage>(u.clone())
                            && let Ok(mut lock) = captured_usage_clone.lock() {
                                *lock = Some(parsed);
                            }

                    // 2. 轉換 text_delta 為乾淨的終端文字輸出
                    if let Some(delta) = val.pointer("/step_update/text_delta").and_then(|v| v.as_str()) {
                        let _ = tx_out.send((StreamChannel::Stdout, delta.to_string())).await;
                    } else if let Some(resp) = val.pointer("/result/response").and_then(|v| v.as_str()) {
                        // 若無 step_update 串流則輸出完整結果
                        let _ = tx_out.send((StreamChannel::Stdout, resp.to_string())).await;
                    }
                } else {
                    // 非 JSON 格式時按原樣輸出
                    if tx_out.send((StreamChannel::Stdout, line)).await.is_err() {
                        break;
                    }
                }
            }
        });

        let tx_err = log_tx.clone();
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx_err.send((StreamChannel::Stderr, line)).await.is_err() {
                    break;
                }
            }
        });

        // 釋放主執行緒持有的 Sender，確保當 stdout_task 與 stderr_task 結束時，log_rx 會收到 EOF
        drop(log_tx);

        // 5. 競爭等待：正常完成 vs 超時 (Timeout) vs 使用者中止 (Abort)
        let timeout_duration = Duration::from_secs(self.req.timeout_secs);
        let (final_state, exit_code) = tokio::select! {
            res = timeout(timeout_duration, child.wait()) => {
                match res {
                    Ok(Ok(status)) => {
                        let code = status.code();
                        let state = if status.success() { TaskState::Completed } else { TaskState::Failed };
                        (state, code)
                    }
                    Ok(Err(e)) => {
                        self.emit_system_log(format!("等待子行程時發生 I/O 錯誤: {}", e));
                        (TaskState::Failed, None)
                    }
                    Err(_) => {
                        // 觸發 Timeout
                        let _ = child.kill().await;
                        self.emit_system_log(format!("任務執行超過 {} 秒，已強制終止 (SIGKILL)。", self.req.timeout_secs));
                        (TaskState::Timeout, None)
                    }
                }
            }
            _ = &mut abort_rx => {
                // 使用者透過 WebSocket 發送 ABORT_TASK
                let _ = child.kill().await;
                self.emit_system_log("收到使用者中止指令，已強制終止 CLI 行程。".to_string());
                (TaskState::Cancelled, None)
            }
        };

        // 6. 確保所有殘留的管道輸出都被讀取完畢並推送至前端
        let _ = tokio::join!(stdout_task, stderr_task);
        let _ = batcher_handle.await;

        // 7. 若任務完成且要求結構化摘要，嘗試讀取並廣播 Artifact
        if final_state == TaskState::Completed && self.req.require_json_summary {
            self.try_harvest_artifact(&work_dir).await;
        }

        // 自動捕捉 Git Diff 並推送
        let diff_report = crate::executor::git::GitInspector::capture_workspace_diff(&work_dir).await;
        if diff_report.is_git_repo && !diff_report.changed_files.is_empty() {
            let _ = self.event_tx.send(ServerEvent::ArtifactReady {
                task_id: self.task_id,
                artifact: ArtifactData {
                    file_name: "git-diff.patch".to_string(),
                    artifact_type: ArtifactType::Diff,
                    content: serde_json::to_value(&diff_report).unwrap_or_default(),
                },
            });
        }

        let token_usage = captured_usage.lock().ok().and_then(|mut guard| guard.take());

        // 8. 廣播最終狀態轉換
        let _ = self.event_tx.send(ServerEvent::TaskStateChanged {
            task_id: self.task_id,
            old_state: Some(TaskState::Running),
            new_state: final_state,
            exit_code,
            timestamp: Utc::now(),
            usage: token_usage,
        });

        final_state
    }

    /// 50ms 視窗緩衝聚合器 (Throttling & Batching)
    async fn run_log_batcher(
        task_id: Uuid,
        mut rx: mpsc::Receiver<(StreamChannel, String)>,
        event_tx: broadcast::Sender<ServerEvent>,
    ) {
        let mut ticker = interval(Duration::from_millis(50));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut buffer: Vec<LogEntry> = Vec::with_capacity(128);
        let seq_counter = AtomicU64::new(1);

        loop {
            tokio::select! {
                maybe_item = rx.recv() => {
                    match maybe_item {
                        Some((channel, line)) => {
                            buffer.push(LogEntry {
                                seq: seq_counter.fetch_add(1, Ordering::Relaxed),
                                channel,
                                line,
                                timestamp: Utc::now(),
                            });
                            // 防護機制：若 50ms 內瞬間湧入超過 250 行，立即強制 Flush 避免記憶體暴增
                            if buffer.len() >= 250 {
                                let logs = std::mem::replace(&mut buffer, Vec::with_capacity(128));
                                let _ = event_tx.send(ServerEvent::StreamBatch { task_id, logs });
                            }
                        }
                        None => {
                            // 管道已關閉 (子行程結束)，將最後殘留的 Buffer 推出後退出迴圈
                            if !buffer.is_empty() {
                                let _ = event_tx.send(ServerEvent::StreamBatch { task_id, logs: buffer });
                            }
                            break;
                        }
                    }
                }
                _ = ticker.tick() => {
                    if !buffer.is_empty() {
                        let logs = std::mem::replace(&mut buffer, Vec::with_capacity(128));
                        let _ = event_tx.send(ServerEvent::StreamBatch { task_id, logs });
                    }
                }
            }
        }
    }

    /// 驗證工作目錄是否合法且位於允許的 Base Workspace 之下
    fn resolve_safe_workspace(&self) -> Result<PathBuf, String> {
        let base = std::fs::canonicalize(&self.base_workspace)
            .map_err(|e| format!("系統基準目錄無效: {}", e))?;

        let target_raw = if self.req.workspace_path.trim().is_empty() || self.req.workspace_path == "." {
            base.clone()
        } else {
            base.join(&self.req.workspace_path)
        };

        if !target_raw.exists() {
            let _ = std::fs::create_dir_all(&target_raw);
        }

        let target = std::fs::canonicalize(&target_raw)
            .map_err(|e| format!("指定的目標目錄不存在或無法存取 ({:?}): {}", target_raw, e))?;

        if !target.starts_with(&base) {
            return Err("偵測到越權路徑存取 (Path Traversal Blocked)".to_string());
        }
        Ok(target)
    }

    /// 建立 `.antigravity_out` 並附加結構化輸出指令
    async fn prepare_prompt_and_output_dir(&self, work_dir: &Path) -> String {
        if !self.req.require_json_summary {
            return self.req.prompt.clone();
        }

        let out_file = work_dir.join(SUMMARY_REL_PATH);
        if let Some(parent) = out_file.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        // 清除前次殘留的檔案
        let _ = tokio::fs::remove_file(&out_file).await;

        format!(
            "{}\n\n[SYSTEM INSTRUCTION]: Upon completing all actions, write a structured JSON summary (including \"status\", \"summary\", and \"modified_files\") to `{}`.",
            self.req.prompt, SUMMARY_REL_PATH
        )
    }

    /// 檢查並解析 CLI 產出的結構化結果檔
    async fn try_harvest_artifact(&self, work_dir: &Path) {
        let out_file = work_dir.join(SUMMARY_REL_PATH);
        if let Ok(raw_text) = tokio::fs::read_to_string(&out_file).await {
            let (artifact_type, content) = match serde_json::from_str::<serde_json::Value>(&raw_text) {
                Ok(json_val) => (ArtifactType::Json, json_val),
                Err(_) => (ArtifactType::PlainText, serde_json::Value::String(raw_text)),
            };

            let _ = self.event_tx.send(ServerEvent::ArtifactReady {
                task_id: self.task_id,
                artifact: ArtifactData {
                    file_name: SUMMARY_REL_PATH.to_string(),
                    artifact_type,
                    content,
                },
            });
        }
    }

    fn emit_system_log(&self, message: String) {
        let _ = self.event_tx.send(ServerEvent::StreamBatch {
            task_id: self.task_id,
            logs: vec![LogEntry {
                seq: 0, // 系統獨立提示
                channel: StreamChannel::System,
                line: message,
                timestamp: Utc::now(),
            }],
        });
    }

    fn emit_error(&self, code: &str, message: &str) {
        let _ = self.event_tx.send(ServerEvent::Error {
            task_id: Some(self.task_id),
            code: code.to_string(),
            message: message.to_string(),
        });
    }
}
