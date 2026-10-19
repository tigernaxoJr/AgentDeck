為了讓網頁控制台具備完整的版本控制能力，我們在 Rust 後端擴充兩個強大的 Git 操作端點：**「還原單一變更檔案 (`checkout`)」**與**「一鍵安全 Commit（自動 `git add` 指定路徑並依任務 Prompt 自動產生 Commit Message）」**，並在前端 `GitDiffViewer.vue` 中加入對應的按鈕與操作互動。

---

## 1. 後端 Git 操作 API 擴充 (`backend-rust/src/api/routes.rs`)

我們新增兩個請求結構與對應的 Axum 路由，確保所有操作都嚴格限制在安全工作目錄內，且不允許進行危險的強制覆寫（例如 `--hard`）。

### A. 定義請求結構

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertFileRequest {
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitChangesRequest {
    pub message: Option<String>,
}

```

### B. 擴充 Axum 路由與處理常式

```rust
// 於 create_router 的 protected_api 中加入：
.route("/tasks/{id}/diff/revert", post(revert_file_handler))
.route("/tasks/{id}/diff/commit", post(commit_changes_handler))

/// POST /api/tasks/{id}/diff/revert - 還原單一修改或刪除的檔案
async fn revert_file_handler(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(payload): Json<RevertFileRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let target_dir = resolve_task_workspace(&state, task_id).await?;

    // 判斷該檔案是已追蹤修改還是未追蹤新檔案 (Untracked)
    let is_untracked = payload.file_path.starts_with("??") || is_file_untracked(&target_dir, &payload.file_path).await;

    let status = if is_untracked {
        // 若為新建立的未追蹤檔案，直接從磁碟刪除
        tokio::fs::remove_file(target_dir.join(&payload.file_path))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    } else {
        // 若為已追蹤檔案，執行 git checkout HEAD -- <path>
        let out = Command::new("git")
            .current_dir(&target_dir)
            .args(["checkout", "HEAD", "--", &payload.file_path])
            .output()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        if out.status.success() {
            Ok(())
        } else {
            Err((
                StatusCode::BAD_REQUEST,
                String::from_utf8_lossy(&out.stderr).to_string(),
            ))
        }
    };

    status?;
    // 回傳最新的 Git Diff 報告供前端即時更新
    let report = crate::executor::git::GitInspector::capture_workspace_diff(&target_dir).await;
    Ok(Json(report))
}

/// POST /api/tasks/{id}/diff/commit - 一鍵將當前變更加入 Git 並 Commit
async fn commit_changes_handler(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(payload): Json<CommitChangesRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let target_dir = resolve_task_workspace(&state, task_id).await?;

    // 1. 取得任務原本的 Prompt 作為預設 Commit Message
    let task = state
        .repo
        .get_task_by_id(task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "找不到任務".to_string()))?;

    let commit_msg = payload
        .message
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| format!("feat(antigravity): apply AI agent changes for task\n\nPrompt: {}", task.prompt));

    // 2. 執行 git add . (排除 .antigravity_out)
    let add_out = Command::new("git")
        .current_dir(&target_dir)
        .args(["add", ".", "--", ".", ":(exclude).antigravity_out"])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !add_out.status.success() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Git Add 失敗: {}", String::from_utf8_lossy(&add_out.stderr)),
        ));
    }

    // 3. 執行 git commit -m "<msg>"
    let commit_out = Command::new("git")
        .current_dir(&target_dir)
        .args(["commit", "-m", &commit_msg])
        .output()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !commit_out.status.success() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Git Commit 失敗: {}", String::from_utf8_lossy(&commit_out.stderr)),
        ));
    }

    let report = crate::executor::git::GitInspector::capture_workspace_diff(&target_dir).await;
    Ok(Json(report))
}

// 輔助函式：共用工作目錄路徑驗證
async fn resolve_task_workspace(state: &AppState, task_id: Uuid) -> Result<std::path::PathBuf, (StatusCode, String)> {
    let task = state
        .repo
        .get_task_by_id(task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "找不到指定的任務".to_string()))?;

    let base = std::fs::canonicalize(state.scheduler.base_workspace())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let target = std::fs::canonicalize(base.join(&task.workspace_path))
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("工作目錄無效: {}", e)))?;

    if !target.starts_with(&base) {
        return Err((StatusCode::FORBIDDEN, "越權路徑存取".to_string()));
    }
    Ok(target)
}

async fn is_file_untracked(dir: &std::path::Path, path: &str) -> bool {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["ls-files", "--error-unmatch", path])
        .output()
        .await;
    match out {
        Ok(o) => !o.status.success(),
        Err(_) => true,
    }
}

```

---

## 2. 前端 Pinia Store 擴充 (`useTaskStore.ts`)

在 `useTaskStore.ts` 中新增兩個非同步動作，供介面呼叫：

```typescript
/** 還原單一檔案變更 */
async function revertFile(taskId: string, filePath: string) {
  const res = await authFetch(`/api/tasks/${taskId}/diff/revert`, {
    method: "POST",
    body: JSON.stringify({ filePath }),
  });
  if (!res.ok) throw new Error(await res.text());
  const report = (await res.json()) as GitDiffReport;
  diffsByTask.value.set(taskId, report);
}

/** 一鍵 Git Commit */
async function commitChanges(taskId: string, message?: string) {
  const res = await authFetch(`/api/tasks/${taskId}/diff/commit`, {
    method: "POST",
    body: JSON.stringify({ message }),
  });
  if (!res.ok) throw new Error(await res.text());
  const report = (await res.json()) as GitDiffReport;
  diffsByTask.value.set(taskId, report);
}
```

---

## 3. 前端 GitDiffViewer 組件介面升級 (`GitDiffViewer.vue`)

我們在左側檔案樹狀選單的每個檔案右側加入「🔄 還原」**按鈕，並在左側欄頂部加入**「✨ 一鍵 Commit 變更」按鈕與訊息自訂彈窗。

```vue
<!-- 1. 於 GitDiffViewer.vue 的左側欄頂部加入 Commit 按鈕： -->
<div class="p-3 border-b border-slate-800 flex items-center justify-between">
  <div class="flex items-center gap-2 text-xs font-mono">
    <span class="text-indigo-400">🌿 {{ report.branch }}</span>
    <span class="px-1.5 py-0.5 rounded bg-slate-800 text-slate-300">
      {{ report.changedFiles.length }} 變更
    </span>
  </div>
  <div class="flex items-center gap-1.5">
    <button
      @click="showCommitModal = true"
      :disabled="report.changedFiles.length === 0"
      class="px-2 py-1 rounded bg-indigo-600 hover:bg-indigo-500 disabled:bg-slate-800 disabled:text-slate-600 text-white text-[11px] font-semibold cursor-pointer transition-colors"
    >
      ✨ Commit
    </button>
    <button
      @click="$emit('refresh')"
      :disabled="loading"
      class="text-xs px-2 py-1 rounded bg-slate-800 hover:bg-slate-700 text-slate-300 cursor-pointer"
    >
      🔄
    </button>
  </div>
</div>

<!-- 2. 於左側變更檔案清單的按鈕內加入「還原」動作： -->
<button
  v-for="file in report.changedFiles"
  :key="file.path"
  @click="scrollToFile(file.path)"
  class="w-full px-3 py-2 text-left hover:bg-slate-800/60 flex items-center justify-between text-xs font-mono transition-colors cursor-pointer group"
>
  <div class="flex items-center gap-2.5 truncate">
    <span
      class="w-5 h-5 rounded flex items-center justify-center text-[10px] font-bold shrink-0"
      :class="statusBadgeClass(file.status)"
    >
      {{ file.status }}
    </span>
    <span class="truncate text-slate-300" :title="file.path">{{ file.path }}</span>
  </div>
  <button
    @.stop="revertFile(file.path)"
    class="opacity-0 group-hover:opacity-100 text-[10px] px-1.5 py-0.5 rounded bg-rose-500/20 hover:bg-rose-600 text-rose-300 hover:text-white transition-opacity"
    title="放棄此檔案變更並還原"
  >
    🔄 還原
  </button>
</button>

<!-- 3. 於組件最外層加入 Git Commit 訊息輸入彈窗 (Modal)： -->
<div
  v-if="showCommitModal"
  class="fixed inset-0 z-50 bg-slate-950/80 backdrop-blur-xs flex items-center justify-center p-4"
>
  <div class="w-full max-w-md bg-slate-900 border border-slate-700 rounded-lg shadow-2xl p-6 space-y-4 font-sans">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-bold text-slate-100">✨ 一鍵 Git Commit 提交變更</h3>
      <button @click="showCommitModal = false" class="text-slate-400 hover:text-white text-xs">✕</button>
    </div>
    <p class="text-xs text-slate-400 leading-relaxed">
      將自動執行 <code class="text-indigo-400 font-mono">git add .</code> 並使用下方訊息進行提交。
    </p>
    <textarea
      v-model="commitMessage"
      rows="3"
      placeholder="輸入 Commit Message (留空則自動以任務 Prompt 產生)..."
      class="w-full bg-slate-950 border border-slate-700 rounded p-3 text-xs font-mono text-slate-100 focus:outline-none focus:border-indigo-500 resize-none"
    ></textarea>
    <div class="flex justify-end gap-2 pt-2">
      <button
        @click="submitCommit"
        :disabled="committing"
        class="px-4 py-1.5 bg-indigo-600 hover:bg-indigo-500 disabled:bg-slate-800 text-white text-xs font-semibold rounded transition-colors cursor-pointer"
      >
        {{ committing ? '提交中...' : '確認 Commit' }}
      </button>
    </div>
  </div>
</div>

```

### Script 補上方法邏輯：

```typescript
const emit = defineEmits<{
  (e: "refresh"): void;
  (e: "revert", filePath: string): void;
  (e: "commit", message?: string): void;
}>();

const showCommitModal = ref(false);
const commitMessage = ref("");
const committing = ref(false);

function revertFile(path: string) {
  if (confirm(`確定要放棄檔案 「${path}」 的所有修改嗎？此操作無法復原。`)) {
    emit("revert", path);
  }
}

async function submitCommit() {
  committing.value = true;
  try {
    emit("commit", commitMessage.value);
    showCommitModal.value = false;
    commitMessage.value = "";
  } finally {
    committing.value = false;
  }
}
```

_(並在父組件或 `AntigravityConsole` 中將 `@revert` 與 `@commit` 事件綁定至 `store.revertFile` 與 `store.commitChanges` 即可)_
