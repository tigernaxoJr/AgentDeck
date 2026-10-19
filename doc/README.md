# 設計文件索引

本目錄是各功能實作前寫的設計稿。實作過程中有調整，**以程式碼為準**；目前的改善進度見根目錄的 [`plan.md`](../plan.md)。

## Tutor（AI 家教複習系統）

| 文件 | 內容 |
|------|------|
| [tutor-design.md](tutor-design.md) | 需求、VSA + DDD 架構、資料表、教學狀態機、`ai-cli` crate、工單 T-01…T-22、測試策略 |

## Console（Web 驅動 Antigravity CLI）

| 文件 | 內容 | 原檔名 |
|------|------|--------|
| [console-architecture.md](console-architecture.md) | 系統架構總覽（SAD） | design.md |
| [console-protocol.md](console-protocol.md) | 前後端 Tagged Union 通訊協定 | design2.md |
| [console-runner-scheduler.md](console-runner-scheduler.md) | TaskRunner 50ms 緩衝、TaskScheduler 並行控制 | design3.md |
| [console-api-websocket.md](console-api-websocket.md) | AppState、REST 路由、WebSocket 訂閱過濾 | design4.md |
| [console-frontend.md](console-frontend.md) | Vue 3 + Pinia + Xterm.js 控制台 | design6.md |
| [console-sqlite.md](console-sqlite.md) | 任務與 Artifact 的 SQLite（WAL）持久化 | design7.md |
| [console-graceful-shutdown.md](console-graceful-shutdown.md) | 三階段優雅關閉、清除孤兒 CLI 行程 | design8.md |
| [console-auth.md](console-auth.md) | API Key 驗證（Bearer / WebSocket `?token=`） | design9.md |
| [console-git-diff.md](console-git-diff.md) | 雙軌 Git 快照（status + diff） | design10.md |
| [console-git-revert-commit.md](console-git-revert-commit.md) | 還原單一檔案、一鍵 Commit | design11.md |

> 與設計稿不同之處：伺服器預設只綁 `127.0.0.1`、CORS 改白名單、前端走同源代理（見 `plan.md` Step 1–2）。
