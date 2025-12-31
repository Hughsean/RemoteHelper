# RemoteHelper 部署指南

## 📦 快速部署

### 前置要求

- ✅ Windows 10/11 或 Windows Server 2016+
- ✅ 管理员权限
- ✅ 已配置好 `config.toml` 在目标目录

### 一键部署

```powershell
# 1. 以管理员身份运行 PowerShell
# 右键点击 PowerShell 图标 → 以管理员身份运行

# 2. 切换到项目目录
cd D:\WorkBench\RemoteHelper

# 3. 执行部署脚本
.\deployment\deploy.ps1 -Install
```

这将自动完成：

- ✅ 下载并安装 NSSM（如果未安装）
- ✅ 编译 Release 版本服务器
- ✅ 部署到 `E:\UserData\.Exe\.RemoteHelper`
- ✅ 创建 Windows 服务（自启动）
- ✅ 启动服务

## 🔧 详细使用

### 安装服务

```powershell
# 默认安装到 E:\UserData\.Exe\.RemoteHelper
.\deployment\deploy.ps1 -Install

# 自定义安装目录
.\deployment\deploy.ps1 -Install -TargetDir "D:\Services\RemoteHelper"

# 自定义服务名称
.\deployment\deploy.ps1 -Install -ServiceName "MyRemoteHelper"

# 强制重新安装（覆盖现有服务）
.\deployment\deploy.ps1 -Install -Force
```

### 管理服务

```powershell
# 查看服务状态
Get-Service RemoteHelperServer

# 启动服务
Start-Service RemoteHelperServer

# 停止服务
Stop-Service RemoteHelperServer

# 重启服务
.\deployment\deploy.ps1 -Restart

# 卸载服务
.\deployment\deploy.ps1 -Uninstall
```

### 查看日志

```powershell
# 服务器主日志
Get-Content E:\UserData\.Exe\.RemoteHelper\logs\server.log -Tail 50 -Wait

# 服务标准输出日志
Get-Content E:\UserData\.Exe\.RemoteHelper\logs\service-stdout.log -Tail 50 -Wait

# 服务错误日志
Get-Content E:\UserData\.Exe\.RemoteHelper\logs\service-stderr.log -Tail 50 -Wait
```

## 📁 目录结构

部署后的目录结构：

```
E:\UserData\.Exe\.RemoteHelper\
├── RemoteHelperServer.exe    # 服务器主程序
├── keygen.exe                 # 密钥生成工具
├── gen_cert.exe              # 证书生成工具
├── test_real_ip.exe          # 客户端测试工具
├── config.toml               # 配置文件（需要提前准备）
└── logs\                     # 日志目录
    ├── server.log           # 服务器日志
    ├── service-stdout.log   # 服务标准输出
    ├── service-stderr.log   # 服务错误输出
    ├── web_tunnel.log       # Web 隧道日志
    └── service_*.log        # 各服务日志
```

## ⚙️ 服务配置

### 服务属性

- **服务名称**: `RemoteHelperServer`
- **显示名称**: `RemoteHelper Server`
- **启动类型**: 自动（系统启动时自动运行）
- **恢复策略**: 失败后 5 秒自动重启
- **日志轮转**: 单个日志文件达到 10MB 时自动轮转

### 修改服务配置

```powershell
# 使用 NSSM 图形界面编辑
nssm edit RemoteHelperServer

# 命令行修改启动延迟（毫秒）
nssm set RemoteHelperServer AppRestartDelay 10000

# 修改日志大小限制（字节）
nssm set RemoteHelperServer AppRotateBytes 20971520  # 20MB
```

## 🔒 安全建议

### 1. 配置文件保护

```powershell
# 限制配置文件访问权限
icacls "E:\UserData\.Exe\.RemoteHelper\config.toml" /inheritance:r
icacls "E:\UserData\.Exe\.RemoteHelper\config.toml" /grant:r "SYSTEM:(F)"
icacls "E:\UserData\.Exe\.RemoteHelper\config.toml" /grant:r "Administrators:(F)"
```

### 2. 防火墙规则

```powershell
# 允许服务器端口（默认 9999）
New-NetFirewallRule -DisplayName "RemoteHelper Server" `
    -Direction Inbound `
    -Protocol TCP `
    -LocalPort 9999 `
    -Action Allow `
    -Profile Any
```

### 3. 定期更新

```powershell
# 更新服务器版本
cd D:\WorkBench\RemoteHelper
git pull
.\deploy.ps1 -Install -Force
```

## 🐛 故障排除

### 服务无法启动

1. **检查日志**：

   ```powershell
   Get-Content E:\UserData\.Exe\.RemoteHelper\logs\service-stderr.log -Tail 100
   ```

2. **手动测试**：

   ```powershell
   cd E:\UserData\.Exe\.RemoteHelper
   .\RemoteHelperServer.exe
   ```

3. **检查配置**：

   ```powershell
   Get-Content E:\UserData\.Exe\.RemoteHelper\config.toml
   ```

### 端口被占用

```powershell
# 查找占用端口 9999 的进程
netstat -ano | findstr :9999

# 结束进程（替换 <PID>）
Stop-Process -Id <PID> -Force
```

### 权限不足

```powershell
# 检查目录权限
icacls "E:\UserData\.Exe\.RemoteHelper"

# 添加完全控制权限
icacls "E:\UserData\.Exe\.RemoteHelper" /grant "NT AUTHORITY\SYSTEM:(OI)(CI)F"
```

### NSSM 下载失败

手动下载：

1. 访问 <https://nssm.cc/download>
2. 下载 `nssm-2.24.zip`
3. 解压并将 `win64\nssm.exe` 复制到 `C:\Windows\System32\`

## 📊 监控和维护

### 系统监控

```powershell
# 查看服务器资源占用
Get-Process RemoteHelperServer | Select-Object Name, CPU, WorkingSet

# 查看端口监听状态
netstat -ano | findstr :9999
```

### 日志清理

```powershell
# 清理旧日志（保留最近 7 天）
$logDir = "E:\UserData\.Exe\.RemoteHelper\logs"
Get-ChildItem $logDir -Filter "*.log.*" | 
    Where-Object { $_.LastWriteTime -lt (Get-Date).AddDays(-7) } | 
    Remove-Item -Force
```

### 备份配置

```powershell
# 备份配置文件
$backupName = "config_backup_$(Get-Date -Format 'yyyyMMdd_HHmmss').toml"
Copy-Item "E:\UserData\.Exe\.RemoteHelper\config.toml" "E:\UserData\.Exe\.RemoteHelper\$backupName"
```

## 🔄 更新流程

### 升级服务器版本

```powershell
# 1. 停止服务
Stop-Service RemoteHelperServer

# 2. 备份当前版本
Copy-Item "E:\UserData\.Exe\.RemoteHelper\RemoteHelperServer.exe" `
    "E:\UserData\.Exe\.RemoteHelper\RemoteHelperServer.exe.backup"

# 3. 重新编译和部署
cd D:\WorkBench\RemoteHelper
git pull
cargo build --release --bin RemoteHelperServer
Copy-Item "target\release\RemoteHelperServer.exe" "E:\UserData\.Exe\.RemoteHelper\" -Force

# 4. 启动服务
Start-Service RemoteHelperServer

# 或使用一键强制重装
.\deploy.ps1 -Install -Force
```

## 📞 支持

如遇到问题：

1. 查看日志文件
2. 检查 [故障排除指南](README.md#troubleshooting)
3. 提交 Issue 到 GitHub

---

**最后更新**: 2026-01-01  
**适用版本**: RemoteHelper v2.0.0+
