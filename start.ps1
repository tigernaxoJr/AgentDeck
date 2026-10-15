# 啟動 Antigravity Web-CLI 控制台 (PowerShell)
$ErrorActionPreference = "Stop"

$RootDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $RootDir

Write-Host "==================================================" -ForegroundColor Cyan
Write-Host "🚀 啟動 Antigravity Web-CLI 控制台 (前後端)" -ForegroundColor Cyan
Write-Host "==================================================" -ForegroundColor Cyan

# 啟動後端
Write-Host "📦 [1/2] 正在啟動後端服務 (Rust Axum on :3000)..." -ForegroundColor Yellow
$BackendJob = Start-Job -ScriptBlock {
    param($Dir)
    Set-Location "$Dir\backend"
    cargo run
} -ArgumentList $RootDir

# 啟動前端
Write-Host "🎨 [2/2] 正在啟動前端服務 (Vue 3 Vite on :5173)..." -ForegroundColor Yellow
$FrontendJob = Start-Job -ScriptBlock {
    param($Dir)
    Set-Location "$Dir\frontend"
    pnpm run dev
} -ArgumentList $RootDir

Write-Host "==================================================" -ForegroundColor Green
Write-Host "✨ 服務啟動完成！" -ForegroundColor Green
Write-Host "🌐 前端介面: http://localhost:5173" -ForegroundColor Green
Write-Host "🔌 後端 API: http://127.0.0.1:3000" -ForegroundColor Green
Write-Host "💡 按下 Ctrl + C 即可一併優雅關閉所有服務" -ForegroundColor Green
Write-Host "==================================================" -ForegroundColor Green

try {
    while ($true) {
        # 串流輸出日誌
        Receive-Job -Job $BackendJob | ForEach-Object { Write-Host "[Backend] $_" -ForegroundColor Gray }
        Receive-Job -Job $FrontendJob | ForEach-Object { Write-Host "[Frontend] $_" -ForegroundColor DarkCyan }

        if ($BackendJob.State -ne "Running" -or $FrontendJob.State -ne "Running") {
            break
        }
        Start-Sleep -Milliseconds 200
    }
}
finally {
    Write-Host "`n⚠️ 正在關閉前後端服務..." -ForegroundColor Yellow
    Stop-Job $BackendJob, $FrontendJob -ErrorAction SilentlyContinue
    Remove-Job $BackendJob, $FrontendJob -Force -ErrorAction SilentlyContinue
    Write-Host "👋 服務已完全關閉。" -ForegroundColor Green
}
