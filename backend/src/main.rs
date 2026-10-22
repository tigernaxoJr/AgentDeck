pub mod tutor;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use console_core::{
    api::routes::cors_origins,
    create_console_router,
    AppState,
    TaskRepository,
    TaskScheduler,
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // 1. 讀取環境變數設定
    let base_workspace = std::env::var("ANTIGRAVITY_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().expect("無法取得當前目錄"));

    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://antigravity_tasks.db".to_string());

    let api_key = std::env::var("ANTIGRAVITY_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        eprintln!("⚠️ [Security] 未設定 ANTIGRAVITY_API_KEY，目前以無驗證開發模式運行！");
    }

    // 2. 初始化 SQLite 儲存層 (含 WAL 模式與孤兒任務修復)
    let repo = TaskRepository::init(&db_url)
        .await
        .expect("無法初始化 SQLite 資料庫");

    // 3. 初始化並行排程器並掛載 SQLite 背景事件同步器
    let max_concurrency = 2;
    let (scheduler, event_rx) = TaskScheduler::new(max_concurrency, base_workspace.clone());
    repo.spawn_event_sink(event_rx);

    // 4. 建立共享狀態與 Axum Router
    let app_state = AppState::new(scheduler, repo.clone(), api_key.clone());
    let scheduler_ref = app_state.scheduler.clone();

    // 5. 初始化 Tutor 子系統（題庫 DB、AI 後端）；失敗即中止，避免只剩 404 的半套服務
    let tutor_data_dir = std::path::absolute(
        std::env::var("TUTOR_DATA_DIR").unwrap_or_else(|_| "./tutor_data".to_string()),
    )
    .expect("無法解析 TUTOR_DATA_DIR");
    println!("📚 Tutor 資料目錄: {}", tutor_data_dir.display());
    let (_tutor_state, tutor_router) =
        match tutor::init(tutor_data_dir, app_state.api_key.clone()).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("❌ Tutor 系統初始化失敗: {e}");
                std::process::exit(1);
            }
        };
    let cors = CorsLayer::new()
        .allow_origin(cors_origins())
        .allow_methods(Any)
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ]);

    let app = Router::new()
        .nest("/api/tutor", tutor_router)
        .nest("/api", create_console_router(app_state))
        .layer(cors);

    // 6. 綁定連接埠並掛載 Graceful Shutdown 訊號監聽
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let ip = host
        .parse::<std::net::IpAddr>()
        .unwrap_or_else(|_| panic!("HOST 不是合法的 IP 位址: {host}"));
    // CLI 以 --dangerously-skip-permissions 執行，對外曝露時必須設定 API Key
    if !ip.is_loopback()
        && api_key.is_empty()
        && std::env::var("ANTIGRAVITY_ALLOW_INSECURE").as_deref() != Ok("1")
    {
        eprintln!(
            "❌ [Security] HOST={host} 會對外曝露服務，請設定 ANTIGRAVITY_API_KEY（或明確設定 ANTIGRAVITY_ALLOW_INSECURE=1）。"
        );
        std::process::exit(1);
    }
    let addr = SocketAddr::from((ip, port));
    println!("🚀 Antigravity Web-CLI Bridge 啟動於 http://{}", addr);
    println!("📂 允許的基準工作目錄: {:?}", base_workspace);
    println!("🗄️ 資料庫路徑: {}", db_url);

    let listener = TcpListener::bind(addr).await.expect("無法綁定連接埠");

    // 啟動 Axum Server，當收到 SIGINT 或 SIGTERM 時停止接受新的 HTTP/WS 連線
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Axum 伺服器執行異常");

    // 7. HTTP Server 停止收單後，給予時間終止所有 CLI 子行程並排空佇列
    scheduler_ref
        .shutdown_and_drain(Duration::from_secs(8))
        .await;

    // 8. 將最後的任務取消狀態寫入 SQLite 並關閉連線池
    repo.close().await;
    println!("👋 服務已完全停止。");
}

/// 跨平台監聽系統終止訊號 (Ctrl+C / SIGINT 與 Unix SIGTERM)
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("無法安裝 Ctrl+C 訊號處理器");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("無法安裝 SIGTERM 訊號處理器")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            println!("\n⚠️ 收到 SIGINT (Ctrl+C) 訊號，準備關閉服務...");
        },
        _ = terminate => {
            println!("\n⚠️ 收到 SIGTERM (Systemd Stop) 訊號，準備關閉服務...");
        },
    }
}
