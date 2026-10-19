# ==========================================
# 1. 前端構建階段 (Vue 3 + Vite + pnpm)
# ==========================================
FROM node:24-alpine AS frontend-builder
WORKDIR /app/frontend

# 安裝 pnpm
RUN corepack enable && corepack prepare pnpm@latest --activate

# 先複製依賴設定檔以利用 Docker 快取層
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

# 複製前端原始碼並建置
COPY frontend/ ./
RUN pnpm run build

# ==========================================
# 2. 後端構建階段 (Rust 2024 + musl/alpine)
# ==========================================
FROM rust:1.90-alpine AS backend-builder
WORKDIR /app/backend

# 安裝 SQLite 與 musl 建置所需依賴
RUN apk add --no-cache musl-dev sqlite-dev build-base

# 預先下載編譯依賴以加速 Docker 構建快取
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/crates ./crates
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# 複製真實原始碼並執行正式 release 編譯
COPY backend/src ./src
# 更新時間戳以觸發 cargo 重新編譯 main crate
RUN touch src/main.rs && cargo build --release

# ==========================================
# 3. 最終執行映像檔 (輕量級 Alpine)
# ==========================================
FROM alpine:3.21 AS runner
WORKDIR /app

# 安裝運行依賴 (SQLite, SSL 憑證, bash, git, curl, ca-certificates 等)
RUN apk add --no-cache \
    ca-certificates \
    sqlite-libs \
    libgcc \
    bash \
    git \
    curl \
    nginx

# 建立預設 workspace 與資料庫目錄
RUN mkdir -p /workspace /data /app/dist

# 複製後端 binary
COPY --from=backend-builder /app/backend/target/release/backend /app/backend-rust

# 複製前端 dist 靜態檔案
COPY --from=frontend-builder /app/frontend/dist /app/dist

# 複製 Nginx 設定與啟動腳本
COPY docker/nginx.conf /etc/nginx/http.d/default.conf
COPY docker/entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh /app/backend-rust

# 環境變數預設值
ENV HOST=127.0.0.1 \
    PORT=3000 \
    ANTIGRAVITY_WORKSPACE=/workspace \
    DATABASE_URL=sqlite:///data/antigravity_tasks.db \
    ANTIGRAVITY_API_KEY=""

EXPOSE 80

VOLUME ["/workspace", "/data"]

ENTRYPOINT ["/app/entrypoint.sh"]
