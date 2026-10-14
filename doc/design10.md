為了讓你在網頁上能完整掌握 Antigravity CLI 到底對專案動了哪些手腳，單純執行 `git diff HEAD` 是不夠的——因為 AI Agent 經常會**新增尚未被 Git 追蹤的檔案（Untracked Files）**，且我們必須排除系統自己注入的 `.antigravity_out/` 摘要目錄。

最穩健的架構是在 Rust 後端設計一個「雙軌快照捕捉器（Dual-Track Git Snapshotter）」：結合 `git status --porcelain`（取得變更檔案統計清單）與 `git diff HEAD` + 未追蹤新檔案合成為標準 Unified Diff，並透過 REST API（按需查看）與 WebSocket（任務完成時自動推送）雙路回傳，前端則以輕量級語法著色器渲染出媲美 GitHub Pull Request 的差異檢視面板。

---

## 1. 擴充通訊協定型別 (`protocol.rs` & `protocol.ts`)

我們定義一個專用的 `GitDiffReport` 結構，同時包含「變更檔案清單（含新增/修改/刪除狀態）」與「完整 Unified Diff 字串」。

### 後端 `backend-rust/src/models/protocol.rs` 新增：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFileStat {
    /// 狀態碼："M" (修改), "A" (新增/未追蹤), "D" (刪除), "R" (重新命名)
    pub status: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffReport {
    pub is_git_repo: bool,
    pub branch: String,
    pub changed_files: Vec<ChangedFileStat>,
    pub unified_diff: String,
}

```

### 前端 `frontend-vue/src/types/protocol.ts` 新增：

```typescript
export interface ChangedFileStat {
  status: "M" | "A" | "D" | "R" | string;
  path: string;
}

export interface GitDiffReport {
  isGitRepo: boolean;
  branch: string;
  changedFiles: ChangedFileStat[];
  unifiedDiff: string;
}
```

---

## 2. 後端 Git Diff 擷取引擎：`backend-rust/src/executor/git.rs`

這個模組專門在指定的安全工作目錄下非同步呼叫 `git` 指令。它具備三個重要設計：

1. **自動過濾 `.antigravity_out**`：利用 Git Pathspec `:(exclude).antigravity_out` 忽略系統產生的摘要檔。
2. **捕獲 Untracked 新檔案**：針對 CLI 新建立的檔案（`??` 狀態），自動透過 `git diff --no-index -- /dev/null <file>` 轉化為標準的新增檔案 Diff 格式。
3. **512KB 安全截斷（Truncation Guard）**：若 CLI 執行了 `npm install` 產生巨量變更，自動截斷超長 Diff，保護前端瀏覽器不卡死。

```rust
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

```

---

## 3. 串接 `runner.rs` 與 REST API 端點 (`routes.rs`)

我們提供兩種取得 Diff 的途徑：

1. **任務執行完畢時自動捕捉**：在 `runner.rs` 任務結束瞬間，若偵測到工作目錄有檔案變更，自動發送 `ArtifactReady (artifact_type: Diff)`。
2. **隨時手動重新整理 (`GET /api/tasks/{id}/diff`)**：讓你在執行期間或事後隨時點按鈕拉取最新 Diff。

### A. 在 `backend-rust/src/api/routes.rs` 新增 REST 路由：

```rust
// 於 create_router 的 protected_api 群組中加入一行：
.route("/tasks/{id}/diff", get(get_task_diff_handler))

/// GET /api/tasks/{id}/diff - 即時擷取該任務工作目錄的 Git Diff
async fn get_task_diff_handler(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<GitDiffReport>, (StatusCode, String)> {
    let task = state
        .repo
        .get_task_by_id(task_id) // 從 SQLite 查詢該任務紀錄
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "找不到指定的任務".to_string()))?;

    // 驗證並組合安全工作目錄路徑
    let base = std::fs::canonicalize(state.scheduler.base_workspace())
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let target = std::fs::canonicalize(base.join(&task.workspace_path))
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("工作目錄無效: {}", e)))?;

    if !target.starts_with(&base) {
        return Err((StatusCode::FORBIDDEN, "越權路徑存取".to_string()));
    }

    let report = crate::executor::git::GitInspector::capture_workspace_diff(&target).await;
    Ok(Json(report))
}

```

---

## 4. 前端 Pinia Store 擴充：`useTaskStore.ts`

在 `useTaskStore.ts` 中加入儲存每個任務 `GitDiffReport` 的 Map，以及呼叫 `/api/tasks/{id}/diff` 的動作：

```typescript
// 於 useTaskStore.ts 新增狀態與方法
import type { GitDiffReport } from "@/types/protocol";

const diffsByTask = ref<Map<string, GitDiffReport>>(new Map());
const isDiffLoading = ref<boolean>(false);

const activeDiff = computed(() =>
  activeTaskId.value
    ? (diffsByTask.value.get(activeTaskId.value) ?? null)
    : null,
);

/** 向後端拉取指定任務工作目錄的最新 Git Diff */
async function fetchTaskDiff(taskId: string) {
  isDiffLoading.value = true;
  try {
    const res = await authFetch(`/api/tasks/${taskId}/diff`);
    if (!res.ok) throw new Error(await res.text());
    const report = (await res.json()) as GitDiffReport;
    diffsByTask.value.set(taskId, report);
  } catch (err) {
    console.error("無法取得 Git Diff:", err);
  } finally {
    isDiffLoading.value = false;
  }
}

// 在 handleServerEvent 的 'TASK_STATE_CHANGED' 分支中：
// 當任務轉為 COMPLETED 時，自動拉取一次最新 Git Diff
if (newState === "COMPLETED" && activeTaskId.value === taskId) {
  fetchTaskDiff(taskId);
}
```

_(別忘了將 `activeDiff`, `isDiffLoading`, `fetchTaskDiff` 加入 Store 的 `return` 物件中)_

---

## 5. 前端視覺化 Git Diff 檢視器組件：`frontend-vue/src/components/GitDiffViewer.vue`

為了不在前端引入龐大的第三方 Diff 套件，我們直接用約 100 行 TypeScript 實作一個支援「左側變更檔案導覽樹 + 右側 Unified Diff 語法高亮（紅綠行號對照）」的原生 Tailwind CSS 組件，點擊左側檔案名稱還能直接平滑捲動（Scroll-to-File）到對應的程式碼段落。

```vue
<template>
  <div class="h-full w-full flex bg-slate-950 text-slate-200 overflow-hidden">
    <!-- 空狀態或非 Git 目錄提示 -->
    <div
      v-if="!report || !report.isGitRepo"
      class="flex-1 flex flex-col items-center justify-center gap-3 text-slate-500 font-mono text-xs"
    >
      <p v-if="loading" class="animate-pulse text-indigo-400">
        正在掃描工作目錄 Git 變更...
      </p>
      <p v-else-if="report && !report.isGitRepo">
        ⚠️ 此任務的工作目錄並非 Git 儲存庫 (No .git found)
      </p>
      <p v-else>尚無 Git Diff 資料，請點擊右上角「重新整理 Diff」進行掃描</p>
      <button
        @click="$emit('refresh')"
        class="px-3 py-1.5 rounded bg-slate-800 hover:bg-slate-700 text-slate-200 text-xs transition-colors cursor-pointer"
      >
        🔄 立即掃描 Git 變更
      </button>
    </div>

    <!-- 主檢視區：左側變更檔案清單 + 右側程式碼差異面板 -->
    <template v-else>
      <!-- 左側：變更檔案統計樹 -->
      <aside
        class="w-72 border-r border-slate-800 bg-slate-900/40 flex flex-col shrink-0"
      >
        <div
          class="p-3 border-b border-slate-800 flex items-center justify-between"
        >
          <div class="flex items-center gap-2 text-xs font-mono">
            <span class="text-indigo-400">🌿 {{ report.branch }}</span>
            <span class="px-1.5 py-0.5 rounded bg-slate-800 text-slate-300">
              {{ report.changedFiles.length }} 檔案變更
            </span>
          </div>
          <button
            @click="$emit('refresh')"
            :disabled="loading"
            class="text-xs px-2 py-1 rounded bg-slate-800 hover:bg-slate-700 text-slate-300 cursor-pointer"
            title="重新掃描工作目錄"
          >
            {{ loading ? "..." : "🔄" }}
          </button>
        </div>

        <div class="flex-1 overflow-y-auto divide-y divide-slate-800/40">
          <button
            v-for="file in report.changedFiles"
            :key="file.path"
            @click="scrollToFile(file.path)"
            class="w-full px-3 py-2 text-left hover:bg-slate-800/60 flex items-center gap-2.5 text-xs font-mono transition-colors cursor-pointer"
          >
            <span
              class="w-5 h-5 rounded flex items-center justify-center text-[10px] font-bold shrink-0"
              :class="statusBadgeClass(file.status)"
            >
              {{ file.status }}
            </span>
            <span class="truncate text-slate-300" :title="file.path">{{
              file.path
            }}</span>
          </button>

          <div
            v-if="report.changedFiles.length === 0"
            class="p-6 text-center text-xs text-slate-500 font-mono"
          >
            工作樹乾淨 (Working tree clean)
          </div>
        </div>
      </aside>

      <!-- 右側：Unified Diff 分塊渲染區 -->
      <div class="flex-1 overflow-y-auto p-4 space-y-6 font-mono text-xs">
        <div
          v-for="block in parsedBlocks"
          :key="block.filePath"
          :id="'diff-file-' + block.filePath"
          class="border border-slate-800 rounded-lg overflow-hidden bg-slate-900/30"
        >
          <!-- 檔案標頭列 -->
          <div
            class="px-4 py-2 bg-slate-900 border-b border-slate-800 flex items-center justify-between"
          >
            <span class="font-bold text-slate-200">{{ block.filePath }}</span>
            <div class="flex items-center gap-2 text-[11px]">
              <span class="text-emerald-400">+{{ block.additions }}</span>
              <span class="text-rose-400">-{{ block.deletions }}</span>
            </div>
          </div>

          <!-- 逐行程式碼與顏色高亮 -->
          <div class="overflow-x-auto leading-5">
            <div
              v-for="(line, idx) in block.lines"
              :key="idx"
              class="px-4 whitespace-pre flex"
              :class="lineRowClass(line.type)"
            >
              <span class="w-6 select-none opacity-50 shrink-0">{{
                line.prefix
              }}</span>
              <span class="flex-1">{{ line.content }}</span>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import type { GitDiffReport } from "@/types/protocol";

const props = defineProps<{
  report: GitDiffReport | null;
  loading: boolean;
}>();

defineEmits<{
  (e: "refresh"): void;
}>();

interface DiffLine {
  type: "add" | "del" | "hunk" | "meta" | "context";
  prefix: string;
  content: string;
}

interface FileDiffBlock {
  filePath: string;
  additions: number;
  deletions: number;
  lines: DiffLine[];
}

/** 將原始 Unified Diff 字串解析為結構化的檔案區塊陣列 */
const parsedBlocks = computed<FileDiffBlock[]>(() => {
  if (!props.report?.unifiedDiff) return [];

  const rawChunks = props.report.unifiedDiff
    .split(/^diff --git /m)
    .filter(Boolean);
  return rawChunks.map((chunk) => {
    const rawLines = chunk.split("\n");
    const headerLine = rawLines[0] ?? "";
    // 從 "a/src/main.rs b/src/main.rs" 提取目標路徑
    const match = headerLine.match(/b\/(.+)$/);
    const filePath = match ? match[1] : headerLine;

    let additions = 0;
    let deletions = 0;
    const lines: DiffLine[] = [];

    for (let i = 1; i < rawLines.length; i++) {
      const l = rawLines[i];
      if (
        l.startsWith("+++") ||
        l.startsWith("---") ||
        l.startsWith("index ") ||
        l.startsWith("new file")
      ) {
        continue; // 略過冗餘 Git 標頭，保持畫面乾淨
      } else if (l.startsWith("@@")) {
        lines.push({ type: "hunk", prefix: "@@", content: l });
      } else if (l.startsWith("+")) {
        additions++;
        lines.push({ type: "add", prefix: "+", content: l.slice(1) });
      } else if (l.startsWith("-")) {
        deletions++;
        lines.push({ type: "del", prefix: "-", content: l.slice(1) });
      } else {
        lines.push({
          type: "context",
          prefix: " ",
          content: l.startsWith(" ") ? l.slice(1) : l,
        });
      }
    }

    return { filePath, additions, deletions, lines };
  });
});

function scrollToFile(path: string) {
  const el = document.getElementById("diff-file-" + path);
  el?.scrollIntoView({ behavior: "smooth", block: "start" });
}

function statusBadgeClass(status: string): string {
  switch (status) {
    case "A":
      return "bg-emerald-500/20 text-emerald-400 border border-emerald-500/40";
    case "D":
      return "bg-rose-500/20 text-rose-400 border border-rose-500/40";
    case "R":
      return "bg-purple-500/20 text-purple-400 border border-purple-500/40";
    default:
      return "bg-amber-500/20 text-amber-400 border border-amber-500/40";
  }
}

function lineRowClass(type: DiffLine["type"]): string {
  switch (type) {
    case "add":
      return "bg-emerald-950/50 text-emerald-200";
    case "del":
      return "bg-rose-950/50 text-rose-200";
    case "hunk":
      return "bg-indigo-950/40 text-indigo-300 py-0.5 my-1 border-y border-indigo-900/40";
    default:
      return "text-slate-400";
  }
}
</script>
```

---

## 6. 掛載至主控台 `AntigravityConsole.vue`

最後，只需在 `AntigravityConsole.vue` 的中間控制列新增第 3 個分頁按鈕 **「🔀 程式碼變更 (Git Diff)」**，並在底部顯示區引入 `<GitDiffViewer>` 即可：

```vue
<!-- 1. 於 AntigravityConsole.vue 的 <header> 分頁按鈕群組中加入： -->
<button
  @click="openDiffTab"
  class="px-3 py-1 text-xs font-medium rounded transition-colors flex items-center gap-1.5 cursor-pointer"
  :class="
    activeTab === 'diff'
      ? 'bg-slate-800 text-white'
      : 'text-slate-400 hover:text-slate-200'
  "
>
  <span>🔀 程式碼變更 (Git Diff)</span>
  <span
    v-if="store.activeDiff?.changedFiles.length"
    class="px-1.5 py-0.2 rounded-full text-[10px] font-mono bg-indigo-500/30 text-indigo-300"
  >
    {{ store.activeDiff.changedFiles.length }}
  </span>
</button>

<!-- 2. 於底部顯示區新增第 3 個分頁容器： -->
<GitDiffViewer
  v-if="activeTab === 'diff'"
  :report="store.activeDiff"
  :loading="store.isDiffLoading"
  @refresh="store.activeTaskId && store.fetchTaskDiff(store.activeTaskId)"
/>
```

並在 `<script setup lang="ts">` 補上切換分頁時自動載入的邏輯：

```typescript
import GitDiffViewer from "@/components/GitDiffViewer.vue";

// 將 activeTab 型別擴充為 'terminal' | 'artifact' | 'diff'
const activeTab = ref<"terminal" | "artifact" | "diff">("terminal");

function openDiffTab() {
  activeTab.value = "diff";
  if (store.activeTaskId) {
    store.fetchTaskDiff(store.activeTaskId);
  }
}
```
