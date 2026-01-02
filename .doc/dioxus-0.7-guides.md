# Dioxus 0.7 指南 (Guides)

这些指南建立在[核心概念](https://dioxuslabs.com/learn/0.7/essentials/)章节涵盖的主题之上，提供了更详细的解释和示例。虽然我们建议每个人在学习 Dioxus 时阅读完整的[教程](https://dioxuslabs.com/learn/0.7/tutorial/)或[核心概念](https://dioxuslabs.com/learn/0.7/essentials/)章节，但这些指南旨在供用户根据需要深入了解特定主题。

## 目录

1. [工具 (Tools)](#工具-tools)
2. [平台支持 (Platform Support)](#平台支持-platform-support)
3. [发布 (Publishing)](#发布-publishing)
4. [测试与调试 (Testing and Debugging)](#测试与调试-testing-and-debugging)
5. [实用工具 (Utilities)](#实用工具-utilities)
6. [深入 (In-Depth)](#深入-in-depth)

---

## 工具 (Tools)

Dioxus 提供了一套工具来简化跨平台应用程序的构建。`dioxus-cli` 是构建、运行和管理 Dioxus 应用程序的核心工具。

### 安装 CLI (Installing the CLI)

CLI 捆绑了许多用于 Dioxus 开发的不同工具。Dioxus 为 Windows、macOS 和 Linux 提供了预构建的二进制文件，你可以使用 [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) 下载：

```bash
cargo binstall dioxus-cli
```

#### 手动安装

如果你没有安装 `cargo-binstall` 或者我们没有为你所在的平台提供预构建的二进制文件，你可以使用 `cargo install` 安装 CLI：

```bash
cargo install dioxus-cli
```

### 命令 (Commands)

要验证安装并获取所有可用命令的概览，请运行：

```bash
dx --help
```

你应该会看到类似这样的输出：

```text
Dioxus: build web, desktop, and mobile apps with a single codebase

Usage: dx [OPTIONS] <COMMAND>

Commands:
  new          Create a new Dioxus project
  serve        Build, watch, and serve the project
  bundle       Bundle the Dioxus app into a shippable object
  build        Build the Dioxus project and all of its assets
  run          Run the project without any hotreloading
  init         Init a new project for Dioxus in the current directory
  doctor       Diagnose installed tools and system configuration
  print        Print project information
  translate    Translate a source file into Dioxus code
  fmt          Automatically format RSX
  check        Check the project for any issues
  config       Dioxus config file controls
  self-update  Update the Dioxus CLI to the latest version
  tools        Run a dioxus build tool
  components   Manage components from the dioxus-component registry
  help         Print this message or the help of the given subcommand(s)
```

---

## 平台支持 (Platform Support)

Dioxus 中的大多数代码都可以在所有平台上运行。但是，每个平台都有其自己的一套功能和限制。例如，Web 平台支持通过 `wasm-bindgen` 调用 JavaScript，而桌面平台支持自定义应用程序周围窗口的外观。

### 平台特定代码 (Platform specific code)

在深入研究平台特定代码之前，了解 Rust 中的 features（特性）非常重要。Features 允许你添加仅在启用特定 feature 时才会包含在构建中的代码。

Dioxus 为每个平台使用不同的 feature。例如，Web 平台启用 `dioxus/web` feature，而桌面平台启用 `dioxus/desktop` feature。

在你的 `Cargo.toml` 文件中，你应该为你想要支持的每个平台设置一个 feature。例如：

```toml
[features]
default = []
web = ["dioxus/web"] # This feature is enabled during web builds
desktop = ["dioxus/desktop"] # This feature is enabled during desktop builds
```

设置好 features 后，你可以将代码包装在 `#[cfg(feature = "web")]` 或 `#[cfg(feature = "desktop")]` 中，以便有条件地包含每个平台的代码。

```rust
#[cfg(feature = "web")]
fn web_specific_code() {
    // Code specific to the web platform
}

#[cfg(feature = "desktop")]
fn desktop_specific_code() {
    // Code specific to the desktop platform
}
```

### 平台特定依赖 (Platform-specific dependencies)

除了平台特定代码外，你可能还需要设置仅在特定平台上包含的依赖项。例如，如果你正在构建 Web 应用程序，你可能希望包含对 `wasm-bindgen` crate 的依赖，以便从 Rust 调用 JavaScript 函数。

你可以将平台特定依赖项作为可选依赖项添加到 `Cargo.toml` 中，并使用 `features` 表启用它们。例如：

```toml
# Since this dependency is optional, it isn't included in the build automatically
wasm-bindgen = { version = "*", optional = true }

[features]
default = []
# adding dep:wasm-bindgen enables the wasm-bindgen dependency only in web builds
web = ["dioxus/web", "dep:wasm-bindgen"]
desktop = ["dioxus/desktop"]
```

---

## 发布 (Publishing)

构建应用程序后，你需要将其发布到某个地方。本参考将概述发布桌面或 Web 应用程序的不同方法。

### Web: 使用 GitHub Pages 发布

编辑你的 `Dioxus.toml`，将 `out_dir` 指向 `docs` 文件夹，并将 `base_path` 指向你的仓库名称：

```toml
[application]
# ...
[web.app]
base_path = "your_repo"
```

然后构建你的应用并发布到 Github：

1. 确保 GitHub Pages 已设置为你的仓库发布 `docs` 目录中的任何静态文件。
2. 将你的应用构建到 `docs` 目录中：

   ```bash
   dx bundle --out-dir docs
   ```

3. 将静态内容从 `docs/public` 移动到 `docs`：

   ```bash
   mv docs/public/* docs
   ```

4. 复制你的 `docs/index.html` 文件并将副本重命名为 `docs/404.html`，以便你的应用可以与客户端路由一起工作：

   ```bash
   cp docs/index.html docs/404.html
   ```

5. 添加并提交 git 更改，然后推送到 GitHub。

### Desktop: 创建安装程序

Dioxus 桌面应用使用操作系统的 WebView 库，因此它可以移植并分发到其他平台。

#### 准备应用进行打包

根据你的平台，你可能需要在 `main.rs` 文件中添加一些额外的代码，以确保你的应用准备好进行打包。在 Windows 上，你需要将 `#![windows_subsystem = "windows"]` 属性添加到你的 `main.rs` 文件中，以隐藏运行应用时弹出的终端窗口。如果你在 Windows 上开发，请仅在打包时使用此属性。它将禁用终端，因此你将无法获得任何类型的日志。你可以将其放在一个 feature 后面，如下所示：

`Cargo.toml`:

```toml
[features]
bundle = []
```

`main.rs`:

```rust
#![cfg_attr(feature = "bundle", windows_subsystem = "windows")]
```

#### 向应用添加资源

如果你想将资源与应用程序捆绑在一起，你可以使用 `manganis` crate，或者将它们包含在你的 `Dioxus.toml` 文件中：

```toml
[bundle]
# The list of files to include in the bundle. These can contain globs.
resources = ["main.css", "header.svg", "**/*.png"]
```

#### 构建 (Building)

要打包你的应用程序，你可以简单地运行 `dx bundle --release`（如果你使用了 `bundle` feature，请添加 `--features bundle`）来生成一个包含所有优化和内置资源的最终应用程序。

运行命令后，你的应用程序应该可以在 `dist/bundle/` 中访问。

---

## 测试与调试 (Testing and Debugging)

测试和调试是开发过程的重要组成部分。

### 测试 (Testing)

测试可以分为几类：

* **单元测试 (Unit Testing)**：隔离测试单个组件或函数。
* **端到端测试 (End-to-End Testing)**：从头到尾测试整个应用程序。

#### 单元测试 (Unit Testing)

Rust 内置了使用 `cargo test` 进行测试的支持。你可以使用 `#[test]` 属性注释任何函数以将其标记为测试函数。

```rust
fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn test_add() {
    assert_eq!(add(2, 3), 5);
}
```

#### 端到端测试 (End-to-End Testing)

端到端测试涉及从头到尾测试整个应用程序。对于 Dioxus 应用程序，可以使用 Playwright 来自动化浏览器并测试应用程序的不同方面。

### 避免常见陷阱

虽然了解如何在问题出现时进行调试很重要，但完全避免问题更好。[反模式 (Anti-patterns)](https://dioxuslabs.com/learn/0.7/guides/tips/antipatterns) 指南介绍了一些常见的陷阱以及如何避免它们。

### 优化构建

一旦你解决了应用程序中的所有错误并准备部署，值得花一些时间让你的应用程序为生产做好准备。[优化 (Optimizing)](https://dioxuslabs.com/learn/0.7/guides/tips/optimizing) 指南涵盖了针对生产优化构建设置。

---

## 实用工具 (Utilities)

实用工具部分提供了可与 Dioxus 一起使用的有用库的集合。

* **[日志 (Logging)](https://dioxuslabs.com/learn/0.7/guides/utilities/logging)**：涵盖如何使用 `dioxus-logger` 为你的项目配置日志记录。
* **[国际化 (Internationalization)](https://dioxuslabs.com/learn/0.7/guides/utilities/internationalization)**：解释如何使用 `dioxus-i18n` 在你的应用程序中支持多种语言。
* **[Tailwind](https://dioxuslabs.com/learn/0.7/guides/utilities/tailwind)**：提供将 Dioxus 与 Tailwind CSS 一起使用的设置说明。

---

## 深入 (In-Depth)

如果你准备深入研究为 Dioxus 构建自己的渲染器，[自定义渲染器指南](https://dioxuslabs.com/learn/0.7/guides/depth/custom_renderer) 提供了如何开始的概述。
