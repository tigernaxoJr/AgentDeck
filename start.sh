#!/usr/bin/env bash
set -e

# 切換至專案根目錄
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

echo "=================================================="
echo "🚀 啟動 Antigravity Web-CLI 控制台 (前後端)"
echo "=================================================="

# 1. 定義退場與清理邏輯 (Trap SIGINT / SIGTERM)
cleanup() {
    echo ""
    echo "⚠️ 收到中斷訊號，正在關閉前後端服務..."

    if [ -n "$FRONTEND_PID" ] && kill -0 "$FRONTEND_PID" 2>/dev/null; then
        echo "🛑 關閉前端 (PID: $FRONTEND_PID)..."
        kill "$FRONTEND_PID" 2>/dev/null || true
    fi

    if [ -n "$BACKEND_PID" ] && kill -0 "$BACKEND_PID" 2>/dev/null; then
        echo "🛑 關閉後端 (PID: $BACKEND_PID)..."
        # 發送 SIGTERM 給 Rust 後端，觸發三階段優雅退場
        kill "$BACKEND_PID" 2>/dev/null || true
    fi

    # 等待子行程清理完畢
    wait 2>/dev/null || true
    echo "👋 前後端服務已全數安全關閉。"
    exit 0
}

trap cleanup SIGINT SIGTERM EXIT

# 2. 啟動後端 (Rust Axum)
echo "📦 [1/2] 正在啟動後端服務 (Rust Axum on :3000)..."
cd "$ROOT_DIR/backend-rust"
cargo run &
BACKEND_PID=$!

# 3. 啟動前端 (Vue 3 + Vite)
echo "🎨 [2/2] 正在啟動前端服務 (Vue 3 Vite on :5173)..."
cd "$ROOT_DIR/frontend-vue"
npm run dev &
FRONTEND_PID=$!

echo "=================================================="
echo "✨ 服務啟動完成！"
echo "🌐 前端介面: http://localhost:5173"
echo "🔌 後端 API: http://127.0.0.1:3000"
echo "💡 按下 Ctrl + C 即可一併優雅關閉所有服務"
echo "=================================================="

# 等待任一程序退場
wait -n "$BACKEND_PID" "$FRONTEND_PID" 2>/dev/null || wait
