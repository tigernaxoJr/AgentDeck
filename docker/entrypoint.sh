#!/bin/bash
set -e

echo "=========================================="
echo "🚀 啟動 Antigravity CLI Driver 容器服務"
echo "=========================================="

# 啟動 Nginx (背景執行)
nginx

echo "🌐 Nginx 已啟動於 Port 80 (前端 + 反向代理)"

# 啟動 Rust 後端 (前景執行，接管信號與容器生命週期)
exec /app/backend-rust
