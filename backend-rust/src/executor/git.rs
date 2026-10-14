use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use crate::models::protocol::{ChangedFileStat, GitDiffReport};

const MAX_DIFF_BYTES: usize = 512 * 1024; // 限制最大回傳 512KB Diff

pub struct GitInspector;

impl GitInspector {
    /// 擷取指定工作目錄的 Git 狀態與完整 Unified Diff
    pub async fn capture_workspace_diff(work_dir: &Path) -> GitDiffReport {
        // 1. 檢查是否為有效的 Git 儲存庫並取得當前分支名稱
        let branch_out = Command::new("git")
            .current_dir(work_dir)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await;

        let branch = match branch_out {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            _ => {
                return GitDiffReport {
                    is_git_repo: false,
                    branch: String::new(),
                    changed_files: vec![],
                    unified_diff: String::new(),
                };
            }
        };

        // 2. 執行 git status --porcelain 取得所有變更與新增的檔案清單 (排除 .antigravity_out)
        let status_out = Command::new("git")
            .current_dir(work_dir)
            .args([
                "status",
                "--porcelain",
                "-uall",
                "--",
                ".",
                ":(exclude).antigravity_out",
            ])
            .output()
            .await;

        let mut changed_files = Vec::new();
        let mut untracked_paths = Vec::new();

        if let Ok(out) = status_out {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if line.len() < 4 {
                    continue;
                }
                let raw_code = &line[0..2];
                let file_path = line[3..].trim().to_string();

                let status_label = match raw_code {
                    "??" => {
                        untracked_paths.push(file_path.clone());
                        "A"
                    }
                    s if s.contains('D') => "D",
                    s if s.contains('A') => "A",
                    s if s.contains('R') => "R",
                    _ => "M",
                };

                changed_files.push(ChangedFileStat {
                    status: status_label.to_string(),
                    path: file_path,
                });
            }
        }

        // 3. 取得已追蹤檔案 (Tracked Files) 與 HEAD 的差異
        let mut full_diff = String::new();
        if let Ok(diff_out) = Command::new("git")
            .current_dir(work_dir)
            .args([
                "diff",
                "HEAD",
                "--no-color",
                "--unified=3",
                "--",
                ".",
                ":(exclude).antigravity_out",
            ])
            .output()
            .await
        {
            full_diff.push_str(&String::from_utf8_lossy(&diff_out.stdout));
        }

        // 4. 將 CLI 新建立但尚未 git add 的檔案 (Untracked) 轉換為標準 Unified Diff 附加在後
        for rel_path in untracked_paths {
            if full_diff.len() >= MAX_DIFF_BYTES {
                break;
            }
            // git diff --no-index 會回傳 exit code 1 (代表有差異)，屬於正常現象
            if let Ok(untracked_diff) = Command::new("git")
                .current_dir(work_dir)
                .args(["diff", "--no-index", "--no-color", "--", "/dev/null", &rel_path])
                .output()
                .await
            {
                full_diff.push_str(&String::from_utf8_lossy(&untracked_diff.stdout));
            }
        }

        // 5. 超長保護截斷
        if full_diff.len() > MAX_DIFF_BYTES {
            full_diff.truncate(MAX_DIFF_BYTES);
            full_diff.push_str("\n\n... [系統提示：Diff 內容超過 512KB 上限，已自動截斷] ...\n");
        }

        GitDiffReport {
            is_git_repo: true,
            branch,
            changed_files,
            unified_diff: full_diff,
        }
    }
}
