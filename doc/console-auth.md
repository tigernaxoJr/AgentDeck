為了保護本機或內網服務不被惡意網頁（如透過 CSRF 或跨站 WebSocket 劫持 CSWSH）偷調用並消耗你的 Antigravity CLI Token，我們需要在 Axum 實作一套「雙通道驗證中介軟體（Dual-Channel Auth Middleware）」。

由於瀏覽器原生的 `new WebSocket(url)` API **不支援自訂 HTTP `Authorization` Header**，因此最優雅且符合標準的做法是：

- **REST API 請求**：透過標準 HTTP Header `Authorization: Bearer <API_KEY>` 驗證。
- **WebSocket 握手升級**：透過 URL Query Parameter `/api/ws?token=<API_KEY>` 驗證。
- **防時序攻擊（Timing-Attack Protection）**：在 Rust 端使用固定時間比對（Constant-time comparison），防止攻擊者透過回應時間差猜測 Token。

---

## 1. 後端驗證中介軟體：`backend-rust/src/api/auth.rs`

我們在 Axum 中編寫一個統一的 `require_api_key` 中介軟體，同時支援從 `Authorization` Header 或 URL Query `?token=...` 提取金鑰。

在 `Cargo.toml` 中可加入 `subtle = "2.6"`（用於 Constant-time 字串比對）：

```rust
use axum::{
    extract::{Query, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use subtle::ConstantTimeEq;

use crate::api::routes::AppState;

#[derive(Deserialize)]
pub struct WsAuthQuery {
    pub token: Option<String>,
}

/// Axum 驗證中介軟體：同時支援 Bearer Header (REST) 與 ?token= Query (WebSocket)
pub async fn require_api_key(
    State(state): State<AppState>,
    Query(query): Query<WsAuthQuery>,
    req: Request,
    next: Next,
) -> Response {
    // 若環境變數未設定 API Key (空字串)，則視為開發模式直接放行
    if state.api_key.is_empty() {
        return next.run(req).await;
    }

    // 1. 優先嘗試從 Authorization: Bearer <token> 提取
    let header_token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|val| val.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::trim);

    // 2. 次要嘗試從 URL Query (?token=<token>) 提取 (專供 WebSocket 握手使用)
    let provided_token = header_token.or(query.token.as_deref());

    match provided_token {
        Some(token) if constant_time_eq(token.as_bytes(), state.api_key.as_bytes()) => {
            next.run(req).await
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "code": "UNAUTHORIZED",
                "message": "無效或缺失的 API Key，請於前端設定正確的存取金鑰。"
            })),
        )
            .into_response(),
    }
}

/// 固定時間比對，防止 Timing Attack
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

```

---

## 2. 掛載 Middleware 與 Origin 防護：`backend-rust/src/api/routes.rs`

在 `AppState` 中加入 `api_key: Arc<String>`，並將 `/api` 下的所有路由（含 REST 與 `/api/ws`）套用 `axum::middleware::from_fn_with_state`。若未來有使用 `ServeDir` 託管前端靜態 HTML/JS 檔案，靜態檔案會位於外層而不受 API Key 阻擋，讓使用者能正常打開網頁輸入金鑰。

```rust
// 於 routes.rs 更新 AppState 與 create_router
use axum::middleware;
use crate::api::auth::require_api_key;
use crate::storage::db::TaskRepository;

#[derive(Clone)]
pub struct AppState {
    pub scheduler: Arc<TaskScheduler>,
    pub repo: TaskRepository,
    pub api_key: Arc<String>,
}

impl AppState {
    pub fn new(scheduler: TaskScheduler, repo: TaskRepository, api_key: String) -> Self {
        Self {
            scheduler: Arc::new(scheduler),
            repo,
            api_key: Arc::new(api_key),
        }
    }
}

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ]);

    // 受保護的 API 與 WebSocket 路由群組
    let protected_api = Router::new()
        .route("/tasks", post(create_task_handler).get(list_tasks_handler))
        .route("/tasks/{id}", get(get_task_handler))
        .route("/tasks/{id}/cancel", delete(cancel_task_handler))
        .route("/ws", get(ws_handler))
        // 套用 API Key 驗證中介軟體
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_api_key,
        ));

    Router::new()
        .nest("/api", protected_api)
        .layer(cors)
        .with_state(state)
}

```

### 更新 `main.rs` 與 Systemd 環境變數

在 `main.rs` 讀取 `ANTIGRAVITY_API_KEY` 並傳入 `AppState::new`：

```rust
let api_key = std::env::var("ANTIGRAVITY_API_KEY").unwrap_or_default();
if api_key.is_empty() {
    eprintln!("⚠️ [Security] 未設定 ANTIGRAVITY_API_KEY，目前以無驗證開發模式運行！");
}
let app_state = AppState::new(scheduler, repo.clone(), api_key);

```

並在 `~/.config/systemd/user/antigravity-web.service` 中加入一行：

```ini
Environment="ANTIGRAVITY_API_KEY=ag_secret_9f8e7d6c5b4a3f2e1d0c"

```

---

## 3. 前端 Vue 3 整合動態金鑰與 401 攔截：`useTaskStore.ts`

在前端，我們使用 `@vueuse/core` 的 **`useStorage('antigravity_api_key', '')`** 將金鑰保存在瀏覽器的 `localStorage` 中，並提供兩個關鍵機制：

1. **動態 WebSocket URL (`computed`)**：當使用者在介面上修改 API Key 時，`wsUrl` 會自動更新並觸發 `useWebSocket` 重新握手連線。
2. **統一的 `authFetch` 封裝**：自動在所有 REST 請求夾帶 `Authorization: Bearer <token>`，並在收到 `401 Unauthorized` 時觸發 `isAuthRequired = true`，引導使用者輸入金鑰。

```typescript
// 於 frontend-vue/src/stores/useTaskStore.ts 中更新認證相關邏輯
import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { useWebSocket, useStorage } from "@vueuse/core";
// ... (其餘型別匯入同前)

const API_BASE = import.meta.env.VITE_API_BASE || "http://127.0.0.1:3000";

export const useTaskStore = defineStore("taskStore", () => {
  // 1. 持久化儲存 API Key 於瀏覽器 localStorage
  const apiKey = useStorage<string>("antigravity_api_key", "");
  const isAuthError = ref<boolean>(false);

  // ... (tasks, logsByTask, artifactsByTask, activeTaskId, lastError, metrics 定義同前)

  // 2. 動態計算帶 Token 的 WebSocket URL
  // 當 apiKey 改變時，useWebSocket 會自動斷開舊連線並以新 Token 重新握手
  const wsUrl = computed(() => {
    const baseWs = API_BASE.replace(/^http/, "ws") + "/api/ws";
    const tokenParam = encodeURIComponent(apiKey.value.trim());
    return tokenParam ? `${baseWs}?token=${tokenParam}` : baseWs;
  });

  const {
    status: wsStatus,
    send: wsSend,
    open: wsReconnect,
  } = useWebSocket(wsUrl, {
    autoReconnect: {
      retries: 5,
      delay: 2000,
    },
    heartbeat: {
      message: JSON.stringify({ type: "PING" }),
      interval: 15000,
    },
    onConnected() {
      isAuthError.value = false;
      if (activeTaskId.value) {
        sendCommand({
          type: "SUBSCRIBE_TASK",
          payload: { taskId: activeTaskId.value },
        });
      }
    },
    onMessage(_ws, event) {
      try {
        const msg = JSON.parse(event.data) as ServerEvent;
        handleServerEvent(msg);
      } catch (err) {
        console.error("無法解析 WebSocket 訊息:", err);
      }
    },
  });

  // 3. 封裝帶 Bearer Token 的 Fetch 輔助函式
  async function authFetch(
    path: string,
    options: RequestInit = {},
  ): Promise<Response> {
    const headers = new Headers(options.headers);
    if (apiKey.value.trim()) {
      headers.set("Authorization", `Bearer ${apiKey.value.trim()}`);
    }
    if (options.body && !headers.has("Content-Type")) {
      headers.set("Content-Type", "application/json");
    }

    const res = await fetch(`${API_BASE}${path}`, {
      ...options,
      headers,
    });

    if (res.status === 401) {
      isAuthError.value = true;
      lastError.value =
        "[401 UNAUTHORIZED] API Key 驗證失敗，請設定正確的存取金鑰。";
      throw new Error("Unauthorized");
    }

    isAuthError.value = false;
    return res;
  }

  /** 更新金鑰並立即重新載入資料與重連 WebSocket */
  async function updateApiKey(newKey: string) {
    apiKey.value = newKey.trim();
    lastError.value = null;
    await fetchTasks();
    wsReconnect();
  }

  /** 載入所有歷史任務清單 (改用 authFetch) */
  async function fetchTasks() {
    try {
      const res = await authFetch("/api/tasks");
      if (!res.ok) throw new Error("無法載入任務清單");
      const list = (await res.json()) as TaskMetadata[];

      const newMap = new Map<string, TaskMetadata>();
      for (const item of list) {
        newMap.set(item.taskId, item);
      }
      tasks.value = newMap;

      if (!activeTaskId.value && list.length > 0) {
        selectTask(list[0].taskId);
      }
    } catch {
      // 401 錯誤已由 authFetch 攔截並設定 isAuthError
    }
  }

  /** 建立並派發新 CLI 任務 (改用 authFetch) */
  async function createTask(req: CreateTaskRequest): Promise<TaskMetadata> {
    lastError.value = null;
    const res = await authFetch("/api/tasks", {
      method: "POST",
      body: JSON.stringify(req),
    });

    if (!res.ok) {
      const errText = await res.text();
      lastError.value = errText;
      throw new Error(errText);
    }

    const created = (await res.json()) as TaskMetadata;
    tasks.value.set(created.taskId, created);
    logsByTask.value.set(created.taskId, []);
    selectTask(created.taskId);
    return created;
  }

  return {
    apiKey,
    isAuthError,
    updateApiKey,
    // ... (其餘匯出屬性與方法同前)
  };
});
```

---

## 4. 前端金鑰設定彈窗：嵌入 `AntigravityConsole.vue`

在 `AntigravityConsole.vue` 的頂部標題列加入一個「🔑 金鑰設定」按鈕，當後端回傳 `401 Unauthorized`（`store.isAuthError === true`）或使用者點擊按鈕時，顯示 Tailwind CSS 模態框（Modal）供輸入金鑰：

```vue
<!-- 加在 AntigravityConsole.vue 左側欄頂部標題區塊內 -->
<button
  @click="showKeyModal = true"
  class="text-xs px-2 py-1 rounded bg-slate-800 hover:bg-slate-700 text-slate-300 transition-colors cursor-pointer"
  :class="{ 'ring-1 ring-rose-500 text-rose-300': store.isAuthError }"
  title="設定 API 存取金鑰"
>
  🔑 金鑰
</button>

<!-- 加在 AntigravityConsole.vue 最外層 div 結尾前：API Key 驗證模態框 -->
<div
  v-if="showKeyModal || store.isAuthError"
  class="fixed inset-0 z-50 bg-slate-950/80 backdrop-blur-xs flex items-center justify-center p-4"
>
  <div class="w-full max-w-md bg-slate-900 border border-slate-700 rounded-lg shadow-2xl p-6 space-y-4">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-bold text-slate-100 flex items-center gap-2">
        <span>🔐 設定 Antigravity 存取金鑰</span>
      </h3>
      <button
        v-if="!store.isAuthError"
        @click="showKeyModal = false"
        class="text-slate-400 hover:text-white text-xs"
      >
        ✕
      </button>
    </div>

<p class="text-xs text-slate-400 leading-relaxed">
      請輸入後端環境變數 <code class="text-indigo-400 font-mono">ANTIGRAVITY_API_KEY</code> 設定的存取權杖。金鑰僅儲存於您當前瀏覽器的 LocalStorage 中。
    </p>

<input
  v-model="keyInput"
  type="password"
  placeholder="輸入 Bearer API Key..."
  @keydown.enter="saveKey"
  class="w-full bg-slate-950 border border-slate-700 rounded px-3 py-2 text-xs font-mono text-slate-100 focus:outline-none focus:border-indigo-500"
/>

<div class="flex justify-end gap-2 pt-2">
      <button
        @click="saveKey"
        class="px-4 py-1.5 bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-semibold rounded transition-colors cursor-pointer"
      >
        儲存並連線
      </button>
    </div>
```

並在 `<script setup lang="ts">` 中補上對應的狀態綁定：

```typescript
const showKeyModal = ref(false);
const keyInput = ref(store.apiKey);

async function saveKey() {
  await store.updateApiKey(keyInput.value);
  showKeyModal.value = false;
}
```
