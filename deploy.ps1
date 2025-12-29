# deploy.ps1
$TargetDir = "E:\UserData\.Exe\AutoStartHelper"
$SourceExe = "d:\WorkBench\.B\release\AutoStartHelper.exe"
$ConfigFiles = @("config.toml", "email_template.html")

# 1. 检查并创建目标目录
if (-not (Test-Path $TargetDir)) {
    Write-Host "创建目标目录: $TargetDir" -ForegroundColor Cyan
    New-Item -ItemType Directory -Path $TargetDir | Out-Null
}

# 2. 编译 Release 版本
Write-Host "正在编译 Release 版本..." -ForegroundColor Cyan
cargo build --release

if ($LASTEXITCODE -ne 0) {
    Write-Host "编译失败，请检查错误信息。" -ForegroundColor Red
    exit
}

# 3. 复制文件
Write-Host "正在复制文件到 $TargetDir ..." -ForegroundColor Cyan

# 复制可执行文件
Copy-Item -Path $SourceExe -Destination $TargetDir -Force
Write-Host "已复制: AutoStartHelper.exe" -ForegroundColor Green

# 复制配置文件 (如果目标不存在，或者强制覆盖)
foreach ($file in $ConfigFiles) {
    $destPath = Join-Path $TargetDir $file
    if (-not (Test-Path $destPath)) {
        Copy-Item -Path $file -Destination $TargetDir
        Write-Host "已复制: $file" -ForegroundColor Green
    } else {
        Write-Host "跳过: $file (目标已存在，保留原有配置)" -ForegroundColor Yellow
    }
}

Write-Host "`n部署完成！" -ForegroundColor Green
Write-Host "请确保 'frpc.exe' 也在 '$TargetDir' 目录下，或者已添加到系统 PATH 环境变量中。" -ForegroundColor Yellow
