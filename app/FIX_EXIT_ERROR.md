# 修复：应用意外退出 (0xc0000139)

## 问题原因

错误代码 `0xc0000139` (`STATUS_ENTRYPOINT_NOT_FOUND`) 是因为 **缺少 Microsoft Edge WebView2 运行时**。

Dioxus Desktop 使用 WebView2 来渲染桌面应用界面，这是 Windows 上的必需依赖。

---

## 解决方案

### **方法 1: 使用 winget 安装（推荐）**

```powershell
winget install Microsoft.EdgeWebView2Runtime
```

### **方法 2: 手动下载安装**

1. 访问官方下载页面：
   <https://developer.microsoft.com/en-us/microsoft-edge/webview2/>

2. 下载 **Evergreen Standalone Installer**（常青独立安装程序）

3. 运行安装程序

### **方法 3: 使用 Chocolatey**

```powershell
choco install webview2-runtime
```

---

## 验证安装

安装完成后，运行以下命令验证：

```powershell
winget list "Microsoft Edge WebView2"
```

应该看到类似输出：

```
名称                               ID                               版本           源
Microsoft Edge WebView2 Runtime   Microsoft.EdgeWebView2Runtime    131.0.xxx.xx   winget
```

---

## 重新运行应用

安装 WebView2 后，重新运行应用：

```powershell
cd App-D
dx serve
# 或
cargo run
```

---

## 其他已修复的问题

### **Tokio 运行时配置**

已在 `Cargo.toml` 中添加必要的 tokio features：

```toml
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "time"] }
```

这修复了异步运行时相关的潜在问题。

---

## 预期结果

安装 WebView2 后，应用将正常启动并显示登录界面。
