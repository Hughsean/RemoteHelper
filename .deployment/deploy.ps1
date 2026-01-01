# RemoteHelper Server Deployment Script
# 部署到 Windows 服务，使用 NSSM

param(
    [string]$TargetDir = "E:\UserData\.Exe\.RemoteHelper",
    [string]$ServiceName = "A_RemoteHelperServer",
    [switch]$Install,
    [switch]$Uninstall,
    [switch]$Restart,
    [switch]$Force
)

$ErrorActionPreference = "Stop"

# 颜色输出函数
function Write-Info { param([string]$msg) Write-Host "[INFO] $msg" -ForegroundColor Cyan }
function Write-Success { param([string]$msg) Write-Host "[SUCCESS] $msg" -ForegroundColor Green }
function Write-Warning { param([string]$msg) Write-Host "[WARNING] $msg" -ForegroundColor Yellow }
function Write-Error { param([string]$msg) Write-Host "[ERROR] $msg" -ForegroundColor Red }

# 检查管理员权限
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Error "此脚本需要管理员权限运行"
    Write-Info "请右键点击 PowerShell 选择'以管理员身份运行'，然后执行："
    Write-Host "    .\deploy.ps1 -Install" -ForegroundColor Yellow
    exit 1
}

# 项目根目录（脚本在 deployment 文件夹，需要返回上级）
$ScriptDir = $PSScriptRoot
$ProjectRoot = Split-Path $ScriptDir -Parent

# 动态获取 cargo target 目录
Push-Location $ProjectRoot
try {
    $targetDir = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json | Select-Object -ExpandProperty target_directory
    if (-not $targetDir) {
        throw "无法获取 cargo target 目录"
    }
    Write-Verbose "Cargo target 目录: $targetDir"
}
catch {
    # 如果获取失败，使用默认位置
    $targetDir = Join-Path $ProjectRoot "target"
    Write-Verbose "使用默认 target 目录: $targetDir"
}
finally {
    Pop-Location
}

$BinaryPath = Join-Path $targetDir "release\RemoteHelperServer.exe"
$TargetBinary = Join-Path $TargetDir "RemoteHelperServer.exe"
$TargetConfig = Join-Path $TargetDir "config.toml"

Write-Info "RemoteHelper 服务器部署脚本"
Write-Info "目标目录: $TargetDir"
Write-Info "服务名称: $ServiceName"
Write-Host ""

# 函数：检查 NSSM
function Test-NSSM {
    try {
        $null = Get-Command nssm -ErrorAction Stop
        return $true
    }
    catch {
        return $false
    }
}

# 函数：下载 NSSM
function Install-NSSM {
    Write-Info "未检测到 NSSM，正在下载..."

    $nssmUrl = "https://nssm.cc/release/nssm-2.24.zip"
    $nssmZip = Join-Path $env:TEMP "nssm.zip"
    $nssmExtract = Join-Path $env:TEMP "nssm"

    try {
        # 下载
        Write-Info "下载 NSSM..."
        Invoke-WebRequest -Uri $nssmUrl -OutFile $nssmZip -UseBasicParsing

        # 解压
        Write-Info "解压 NSSM..."
        Expand-Archive -Path $nssmZip -DestinationPath $nssmExtract -Force

        # 复制到系统目录
        $nssmExe = Get-ChildItem -Path $nssmExtract -Recurse -Filter "nssm.exe" | Where-Object { $_.Directory.Name -eq "win64" } | Select-Object -First 1
        if ($nssmExe) {
            $systemPath = "$env:SystemRoot\System32"
            Copy-Item -Path $nssmExe.FullName -Destination $systemPath -Force
            Write-Success "NSSM 安装成功"
        }
        else {
            throw "无法找到 NSSM 可执行文件"
        }

        # 清理
        Remove-Item -Path $nssmZip -Force -ErrorAction SilentlyContinue
        Remove-Item -Path $nssmExtract -Recurse -Force -ErrorAction SilentlyContinue
    }
    catch {
        Write-Error "NSSM 安装失败: $_"
        Write-Info "请手动下载 NSSM: https://nssm.cc/download"
        exit 1
    }
}

# 函数：构建项目
function Build-Project {
    Write-Info "开始编译 RemoteHelper 服务器..."

    Push-Location $ProjectRoot
    try {
        # 检查 Rust 环境
        try {
            $null = Get-Command cargo -ErrorAction Stop
        }
        catch {
            Write-Error "未检测到 Rust 环境，请先安装 Rust: https://rustup.rs/"
            exit 1
        }

        # 编译
        Write-Info "执行: cargo build --release --bin RemoteHelperServer"
        cargo build --release --bin RemoteHelperServer

        if ($LASTEXITCODE -ne 0) {
            throw "编译失败"
        }

        if (-not (Test-Path $BinaryPath)) {
            throw "未找到编译输出: $BinaryPath"
        }

        Write-Success "编译成功: $BinaryPath"
    }
    finally {
        Pop-Location
    }
}

# 函数：部署文件
function Deploy-Files {
    Write-Info "部署文件到目标目录..."

    # 创建目标目录
    if (-not (Test-Path $TargetDir)) {
        New-Item -ItemType Directory -Path $TargetDir -Force | Out-Null
        Write-Success "创建目录: $TargetDir"
    }

    # 检查配置文件
    if (-not (Test-Path $TargetConfig)) {
        Write-Warning "目标目录未找到 config.toml"
        $exampleConfig = Join-Path $ProjectRoot "config.example.toml"
        if (Test-Path $exampleConfig) {
            Copy-Item -Path $exampleConfig -Destination $TargetConfig -Force
            Write-Success "已复制示例配置文件，请编辑: $TargetConfig"
        }
        else {
            Write-Error "未找到配置文件，请手动创建: $TargetConfig"
            exit 1
        }
    }
    else {
        Write-Info "配置文件已存在: $TargetConfig"
    }

    # 复制二进制文件
    Write-Info "复制可执行文件..."
    Copy-Item -Path $BinaryPath -Destination $TargetBinary -Force
    Write-Success "部署完成: $TargetBinary"

    # 复制实用工具
    $utils = @("keygen.exe", "gen_cert.exe", "test_real_ip.exe")
    foreach ($util in $utils) {
        $src = Join-Path (Split-Path $BinaryPath) $util
        if (Test-Path $src) {
            $dst = Join-Path $TargetDir $util
            Copy-Item -Path $src -Destination $dst -Force
            Write-Info "复制工具: $util"
        }
    }
}

# 函数：安装服务
function Install-Service {
    Write-Info "安装 Windows 服务..."

    # 检查服务是否已存在
    $existingService = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    if ($existingService) {
        if ($Force) {
            Write-Warning "服务已存在，强制重新安装..."
            Uninstall-Service
        }
        else {
            Write-Error "服务 '$ServiceName' 已存在"
            Write-Info "使用 -Force 参数强制重新安装，或先运行: .\deploy.ps1 -Uninstall"
            exit 1
        }
    }

    # 使用 NSSM 安装服务
    Write-Info "执行: nssm install $ServiceName `"$TargetBinary`""
    & nssm install $ServiceName "$TargetBinary"

    if ($LASTEXITCODE -ne 0) {
        Write-Error "服务安装失败"
        exit 1
    }

    # 设置服务参数
    Write-Info "配置服务参数..."

    # 设置工作目录
    & nssm set $ServiceName AppDirectory "$TargetDir"

    # 设置服务描述
    & nssm set $ServiceName Description "RemoteHelper 远程系统监控与进程管理服务"

    # 设置服务显示名称
    & nssm set $ServiceName DisplayName "$ServiceName"

    # 设置启动类型为自动
    & nssm set $ServiceName Start SERVICE_AUTO_START

    # 设置日志输出
    $logDir = Join-Path $TargetDir "logs"
    if (-not (Test-Path $logDir)) {
        New-Item -ItemType Directory -Path $logDir -Force | Out-Null
    }

    $stdoutLog = Join-Path $logDir "service-stdout.log"
    $stderrLog = Join-Path $logDir "service-stderr.log"

    & nssm set $ServiceName AppStdout "$stdoutLog"
    & nssm set $ServiceName AppStderr "$stderrLog"

    # 设置日志轮转
    & nssm set $ServiceName AppStdoutCreationDisposition 4  # OPEN_ALWAYS
    & nssm set $ServiceName AppStderrCreationDisposition 4
    & nssm set $ServiceName AppRotateFiles 1
    & nssm set $ServiceName AppRotateOnline 1
    & nssm set $ServiceName AppRotateBytes 10485760  # 10MB

    # 设置故障恢复（失败后自动重启）
    & nssm set $ServiceName AppExit Default Restart
    & nssm set $ServiceName AppRestartDelay 5000  # 5秒后重启

    Write-Success "服务安装成功"

    # 启动服务
    Write-Info "启动服务..."
    Start-Service -Name $ServiceName

    # 等待服务启动
    Start-Sleep -Seconds 2

    # 检查服务状态
    $service = Get-Service -Name $ServiceName
    if ($service.Status -eq "Running") {
        Write-Success "服务已启动: $ServiceName"
        Write-Info "服务状态: $($service.Status)"
    }
    else {
        Write-Warning "服务未能成功启动，状态: $($service.Status)"
        Write-Info "请查看日志: $stdoutLog"
    }
}

# 函数：卸载服务
function Uninstall-Service {
    Write-Info "卸载 Windows 服务..."

    # 检查服务是否存在
    $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    if (-not $service) {
        Write-Warning "服务 '$ServiceName' 不存在"
        return
    }

    # 停止服务
    if ($service.Status -eq "Running") {
        Write-Info "停止服务..."
        Stop-Service -Name $ServiceName -Force
        Start-Sleep -Seconds 2
    }

    # 卸载服务
    Write-Info "执行: nssm remove $ServiceName confirm"
    & nssm remove $ServiceName confirm

    if ($LASTEXITCODE -eq 0) {
        Write-Success "服务卸载成功"
    }
    else {
        Write-Error "服务卸载失败"
        exit 1
    }
}

# 函数：重启服务
function Restart-Service {
    Write-Info "重启服务..."

    $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    if (-not $service) {
        Write-Error "服务 '$ServiceName' 不存在"
        exit 1
    }

    Write-Info "停止服务..."
    Stop-Service -Name $ServiceName -Force
    Start-Sleep -Seconds 2

    Write-Info "启动服务..."
    Start-Service -Name $ServiceName
    Start-Sleep -Seconds 2

    $service = Get-Service -Name $ServiceName
    Write-Success "服务状态: $($service.Status)"
}

# 主流程
try {
    # 检查 NSSM
    if (-not (Test-NSSM)) {
        Install-NSSM
    }

    if ($Uninstall) {
        # 仅卸载服务
        Uninstall-Service
        Write-Success "卸载完成"
        exit 0
    }

    if ($Restart) {
        # 仅重启服务
        Restart-Service
        Write-Success "重启完成"
        exit 0
    }

    if ($Install) {
        # 完整部署流程
        Write-Host "========================================" -ForegroundColor Cyan
        Write-Host "  RemoteHelper 服务器部署" -ForegroundColor Cyan
        Write-Host "========================================" -ForegroundColor Cyan
        Write-Host ""

        # 1. 编译项目
        Build-Project
        Write-Host ""

        # 2. 部署文件
        Deploy-Files
        Write-Host ""

        # 3. 安装服务
        Install-Service
        Write-Host ""

        Write-Success "========================================"
        Write-Success "  部署完成！"
        Write-Success "========================================"
        Write-Host ""
        Write-Info "服务信息:"
        Write-Host "  服务名称: $ServiceName" -ForegroundColor White
        Write-Host "  安装位置: $TargetDir" -ForegroundColor White
        Write-Host "  配置文件: $TargetConfig" -ForegroundColor White
        Write-Host ""
        Write-Info "管理命令:"
        Write-Host "  查看状态: Get-Service $ServiceName" -ForegroundColor Yellow
        Write-Host "  启动服务: Start-Service $ServiceName" -ForegroundColor Yellow
        Write-Host "  停止服务: Stop-Service $ServiceName" -ForegroundColor Yellow
        Write-Host "  重启服务: .\deploy.ps1 -Restart" -ForegroundColor Yellow
        Write-Host "  卸载服务: .\deploy.ps1 -Uninstall" -ForegroundColor Yellow
        Write-Host ""
        Write-Info "日志位置:"
        Write-Host "  $TargetDir\logs\" -ForegroundColor White
        Write-Host ""
    }
    else {
        # 未指定操作，显示帮助
        Write-Host "RemoteHelper 服务器部署脚本" -ForegroundColor Cyan
        Write-Host ""
        Write-Host "用法:" -ForegroundColor Yellow
        Write-Host "  .\deploy.ps1 -Install              安装并启动服务"
        Write-Host "  .\deploy.ps1 -Install -Force       强制重新安装"
        Write-Host "  .\deploy.ps1 -Uninstall            卸载服务"
        Write-Host "  .\deploy.ps1 -Restart              重启服务"
        Write-Host ""
        Write-Host "参数:" -ForegroundColor Yellow
        Write-Host "  -TargetDir <路径>                  指定安装目录（默认: E:\UserData\.Exe\.RemoteHelper）"
        Write-Host "  -ServiceName <名称>                指定服务名称（默认: RemoteHelperServer）"
        Write-Host "  -Force                             强制重新安装"
        Write-Host ""
        Write-Host "示例:" -ForegroundColor Yellow
        Write-Host "  .\deploy.ps1 -Install"
        Write-Host "  .\deploy.ps1 -Install -TargetDir 'D:\Services\RemoteHelper'"
        Write-Host "  .\deploy.ps1 -Uninstall"
        Write-Host ""
    }
}
catch {
    Write-Error "部署失败: $_"
    Write-Host $_.ScriptStackTrace -ForegroundColor Red
    exit 1
}
