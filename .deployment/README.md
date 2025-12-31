# RemoteHelper 部署文件

本文件夹包含 RemoteHelper 服务器的部署脚本和相关文档。

## 📁 文件说明

- **deploy.ps1** - Windows 服务自动部署脚本（使用 NSSM）
- **DEPLOY.md** - 详细的部署指南和故障排除文档

## 🚀 快速开始

### 一键部署

```powershell
# 1. 以管理员身份运行 PowerShell
# 2. 切换到项目根目录
cd D:\WorkBench\RemoteHelper

# 3. 执行部署脚本
.\deployment\deploy.ps1 -Install
```

### 常用命令

```powershell
# 安装服务
.\deployment\deploy.ps1 -Install

# 重启服务
.\deployment\deploy.ps1 -Restart

# 卸载服务
.\deployment\deploy.ps1 -Uninstall

# 查看服务状态
Get-Service RemoteHelperServer
```

## 📖 详细文档

完整的部署说明、配置选项和故障排除，请查看 [DEPLOY.md](DEPLOY.md)

## 🔧 自定义部署

```powershell
# 自定义安装目录
.\deployment\deploy.ps1 -Install -TargetDir "D:\Services\RemoteHelper"

# 自定义服务名称
.\deployment\deploy.ps1 -Install -ServiceName "MyRemoteHelper"

# 强制重新安装
.\deployment\deploy.ps1 -Install -Force
```

## ⚠️ 注意事项

1. 脚本需要**管理员权限**运行
2. 确保目标目录已有 `config.toml` 配置文件
3. 首次安装会自动下载 NSSM（如果未安装）
4. 服务将配置为系统启动时自动运行

## 📞 支持

- 部署问题请查看 [DEPLOY.md](DEPLOY.md) 的故障排除章节
- 其他问题请参考项目主 [README.md](../README.md)
