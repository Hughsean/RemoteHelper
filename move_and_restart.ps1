# PowerShell 脚本：关闭服务，移动程序，重启服务
# 请根据实际服务名和路径修改变量

$ServiceName = "FrpcStartup"  # 服务名称
$TargetDir = "E:\UserData\.Exe\StartUp"  # 目标文件夹
$BuildDir = "D:\WorkBench\.B\release"  # 构建输出目录
$ExeName = "FrpcStartup.exe"  # 可执行文件名

Write-Host "停止服务..."
Stop-Service -Name $ServiceName -ErrorAction SilentlyContinue
Start-Sleep -Seconds 3

Write-Host "移动新程序到目标目录..."
$SourceExe = Join-Path $BuildDir $ExeName
$TargetExe = Join-Path $TargetDir $ExeName

if (Test-Path $SourceExe) {
    Move-Item -Force $SourceExe $TargetExe
    Write-Host "已移动 $ExeName 到 $TargetDir"
} else {
    Write-Host "未找到 $SourceExe，移动失败！"
    exit 1
}

Write-Host "启动服务..."
Start-Service -Name $ServiceName
Write-Host "操作完成！"
