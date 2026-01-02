# Dioxus 0.7 参考与迁移 (Reference & Migration)

## 迁移指南 (Migration Guides)

Dioxus 在其生命周期中不断演进。我们认为目前的 API 已经相当稳定，但未来仍可能会有变化。

为了帮助在不同版本之间进行迁移，我们汇编了一套迁移指南：

* [v0.4 到 v0.5](https://dioxuslabs.com/learn/0.7/migration/to_05/)
* [v0.5 到 v0.6](https://dioxuslabs.com/learn/0.7/migration/to_06)
* [v0.6 到 v0.7](https://dioxuslabs.com/learn/0.7/migration/to_07)

---

## 进阶 (Beyond)

本指南在 Dioxus 基础知识之上进行了扩展，深入探讨了框架的内部结构。它涵盖了：

* [为 Dioxus 做贡献](https://dioxuslabs.com/learn/0.7/beyond/contributing)
* [Dioxus 的项目结构](https://dioxuslabs.com/learn/0.7/beyond/project_structure)

除了这些章节之外，阅读 [Dioxus 博客](https://dioxuslabs.com/blog) 中的文章也可能会有所帮助，这些文章讨论了 Dioxus 内部使用的有趣优化，例如 [模板差异比较 (template diffing)](https://dioxuslabs.com/blog/templates-diffing)。

---

## 贡献 (Contributing)

开发工作在 [Dioxus GitHub 仓库](https://github.com/DioxusLabs/dioxus) 中进行。如果你发现了 bug 或有新功能的想法，请提交 issue（但请先检查是否有人[已经提交过](https://github.com/DioxusLabs/dioxus/issues)）。

[GitHub discussions](https://github.com/DioxusLabs/dioxus/discussions) 可以作为一个寻求帮助或讨论特性的地方。你也可以加入[我们的 Discord 频道](https://discord.gg/XgGxMSkvUM)，那里也会进行一些开发讨论。

### 改进文档 (Improving Docs)

如果你想改进文档，欢迎提交 PR！Rust 文档 ([源码](https://github.com/DioxusLabs/dioxus/tree/main/packages)) 和本指南 ([源码](https://github.com/DioxusLabs/docsite/tree/main/docs-src/0.7)) 可以在各自的 GitHub 仓库中找到。

### 致力于生态系统 (Working on the Ecosystem)

React 伟大的部分原因在于其丰富的生态系统。我们希望 Dioxus 也能如此！所以，如果你有一个想要编写的库，并且许多人会从中受益，我们将不胜感激。你可以浏览 [npm.js](https://www.npmjs.com/search?q=keywords:react-component) 寻找灵感。完成后，将你的库添加到 [awesome dioxus](https://github.com/DioxusLabs/awesome-dioxus) 列表中，或者在 [Discord](https://discord.gg/XgGxMSkvUM) 的 `#I-made-a-thing` 频道分享。

### Bug 与特性 (Bugs & Features)

如果你修复了 [一个未解决的 issue](https://github.com/DioxusLabs/dioxus/issues)，请随时提交 PR！考虑先[联系](https://discord.gg/XgGxMSkvUM)团队，确保大家达成共识，避免做无用功！

所有 Pull Request（包括团队成员提交的）必须由至少另一名团队成员批准。关于设计、架构、破坏性变更、权衡等更大、更细微的决定由团队协商一致做出。

### 在你贡献之前 (Before you contribute)

你可能会惊讶地发现，在进行第一次 PR 时，很多检查都会失败。这就是为什么你应该在贡献之前先运行这些命令以节省时间，因为 GitHub CI 执行所有这些命令的速度比你的 PC 慢得多。

* 使用 [rustfmt](https://github.com/rust-lang/rustfmt) 格式化代码：

    ```bash
    cargo fmt -- packages/**/*.rs
    ```

* 在 Linux (Ubuntu/deb) 上，你可能需要安装一些包才能成功完成以下命令（仓库根目录也有一个 Nix flake）：

    ```bash
    sudo apt install libgdk3.0-cil libatk1.0-dev libcairo2-dev libpango1.0-dev \
    libgdk-pixbuf2.0-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
    libwebkit2gtk-4.1-dev
    ```

* 检查所有代码 [cargo check](https://doc.rust-lang.org/cargo/commands/cargo-check.html)：

    ```bash
    cargo check --workspace --examples --tests
    ```

* 检查 [Clippy](https://doc.rust-lang.org/clippy/) 是否生成任何警告。请修复这些警告！

    ```bash
    cargo clippy --workspace --examples --tests -- -D warnings
    ```

* 使用 [cargo-test](https://doc.rust-lang.org/cargo/commands/cargo-test.html) 测试所有代码：

    ```bash
    cargo test --all --tests
    ```

* 使用 Playwright 进行测试。这会在浏览器中直接测试 UI 本身。以下是所有步骤，包括安装：
    *免责声明：这可能会在你的机器上莫名其妙地失败，但这并不是你的错。尽管提交 PR 吧！*

    ```bash
    cd packages/playwright-tests
    npm ci
    npm install -D @playwright/test
    npx playwright test
    ```

### 如何使用本地 crate 测试 Dioxus (How to test dioxus with local crate)

如果你正在开发一个功能，你应该在提交 PR 之前在本地设置中对其进行测试。这个过程确保你在同行评审之前了解你的代码功能。

1. Fork 以下 github 仓库 (DioxusLabs/dioxus)：
    `https://github.com/DioxusLabs/dioxus`

2. 创建一个新的或使用现有的 rust crate（如果你使用现有的 rust crate，请忽略此步骤）：这是我们将测试 fork 的功能的地方。

    ```bash
    cargo new --bin demo
    ```

3. 在 `Cargo.toml` 中将 dioxus 依赖项添加到你的 rust crate（新的/现有的）：

    ```toml
    dioxus = { path = "<path to forked dioxus project>/dioxus/packages/dioxus", features = ["web", "router"] }
    ```

    上面的例子是针对带有 `dioxus-router` 的 `dioxus-web`。要了解不同渲染器的依赖关系，请访问[这里](https://dioxuslabs.com/learn/0.7/getting_started/)。

4. 运行并测试你的功能

    ```bash
    dx serve
    ```

    如果这是你第一次使用 dioxus，请阅读[教程](https://dioxuslabs.com/learn/0.7/tutorial/)以熟悉 dioxus。

---

## 项目结构 (Project Structure)

Dioxus 组织中有许多包。本文档将帮助你了解每个包的用途以及它们如何组合在一起：

![Dioxus 依赖图](https://dioxuslabs.com/assets/workspace-graph-dxhf4f2647e77e91e3.webp)

### 入口点 (Entry Points)

* [dioxus](https://github.com/DioxusLabs/dioxus/tree/main/packages/dioxus): Dioxus 应用程序的主 crate。`dioxus` crate 具有不同的特性标志 (feature flags)，用于通过 `launch` API 启用特定的渲染器，并公开不同的功能，如路由器和全栈。[CLI](https://github.com/DioxusLabs/dioxus/tree/main/packages/cli) 使用启用的渲染器特性标志来确定要编译的 rust 目标。

### 渲染器 (Renderers)

渲染器是 Dioxus 应用程序的入口点。它们处理渲染应用程序、轮询异步任务和处理事件。每个渲染器都依赖于 `dioxus-core` 来获取核心虚拟 DOM，并实现 `dioxus-history` 中的 history trait 和 `dioxus-html` 中的事件转换 trait。Dioxus 主仓库中有四个渲染器：

* [web](https://github.com/DioxusLabs/dioxus/tree/main/packages/web): 通过编译为 WASM 并操作 DOM，在浏览器中渲染 Dioxus 应用程序。如果启用了全栈，web 渲染器具有 hydration（水合）功能，可以从服务器接管渲染。
* [desktop](https://github.com/DioxusLabs/dioxus/tree/main/packages/desktop): 在桌面和移动平台上运行的渲染器。Dioxus 应用程序代码是原生编译的，UI 使用系统 webview 渲染。
* [mobile](https://github.com/DioxusLabs/dioxus/tree/main/packages/mobile): 一个原生运行 Dioxus 应用程序的渲染器，但使用系统 webview 渲染它们。目前这是 desktop 渲染器之上的一个薄封装，因为两个渲染器都使用 webview。
* [native](https://github.com/DioxusLabs/dioxus/tree/main/packages/native): 一个（实验性）渲染器，在桌面和移动平台上运行。Dioxus 应用程序是原生编译的，UI 使用自定义 WGPU HTML/CSS 渲染器 ([blitz](https://github.com/DioxusLabs/blitz)) 渲染。
* [liveview](https://github.com/DioxusLabs/dioxus/tree/main/packages/liveview): 在服务器上运行的渲染器，并在浏览器中使用 websocket 代理进行渲染。Liveview 渲染器目前受支持，但开发优先级已降低，转而支持全栈，未来可能会被移除。

> [TUI](https://github.com/DioxusLabs/blitz/tree/legacy/packages/dioxus-tui) 渲染器已被弃用，但一旦 Blitz 更加稳定，未来可能会重新审视。

### 原生渲染 (Native Rendering)

除了上面列出的渲染器之外，Dioxus 还有一个名为 Blitz 的实验性原生渲染器，它使用 WebGPU 为 dioxus 应用程序渲染 HTML+CSS：

* [taffy](https://github.com/DioxusLabs/taffy): 独立的 CSS 布局引擎，为 Blitz 提供动力（也被 Zed 和 Bevy UI 使用）。
* [blitz](https://github.com/DioxusLabs/blitz): 一个实验性的自定义 WGPU HTML/CSS 渲染器，是 Dioxus Native 的基础。
* [native-dom](https://github.com/DioxusLabs/dioxus/tree/main/packages/native-dom): `blitz` 与 `dioxus-core` 的核心集成。用于将 Dioxus Native 嵌入到另一个已经拥有自己的窗口和输入处理的应用程序（例如 Bevy 游戏）中。

### 全栈 (Fullstack)

全栈可以分层在任何渲染器之上，以添加对服务器函数 (server functions) 和服务端渲染 (SSR) 的支持。

* [ssr](https://github.com/DioxusLabs/dioxus/tree/main/packages/ssr): `dioxus-ssr` 处理将 dioxus 虚拟 DOM 渲染为字符串，用于测试或在服务器上使用。SSR 在全栈渲染器中用于处理服务端渲染和静态生成。
* [isrg](https://github.com/DioxusLabs/dioxus/tree/main/packages/isrg): `dioxus-isrg` 处理 dioxus 全栈应用程序的增量静态站点生成。它帮助全栈在内存和文件系统中缓存服务端渲染的路由。
* [fullstack](https://github.com/DioxusLabs/dioxus/tree/main/packages/fullstack): `dioxus-fullstack` 包处理 [axum](https://github.com/tokio-rs/axum) 服务器和 dioxus 渲染器之间的集成。如果前端渲染器以 web 为目标，全栈渲染器将准备带有嵌入数据的 html，以便客户端可以在初始加载（水合）后接管渲染。
* [server-macro](https://github.com/DioxusLabs/dioxus/tree/main/packages/server-macro): `server-macro` crate 定义了 `server` 宏，用于在 Dioxus 应用程序中定义服务器函数。它与 [server_fn](https://crates.io/crates/server_fn) 集成，以自动在服务器上注册服务器函数并在客户端调用它们。

### 核心工具 (Core utilities)

核心工具包含虚拟 DOM 的实现，以及所有 dioxus 渲染器中使用的其他宏。Dioxus 的核心不假设它在 web 上下文中运行，因此这些工具可以被第三方渲染器（如 [Freya](https://github.com/marc2332/freya)）使用。

* [core](https://github.com/DioxusLabs/dioxus/tree/main/packages/core): 每个 Dioxus 应用程序使用的核心虚拟 DOM 实现。core 的主要入口点是 `VirtualDom`。虚拟 DOM diff 方法接受一个跨平台的 `WriteMutations` trait，该 trait 在渲染器需要更改渲染内容时被调用。vdom 也有运行 future 和插入事件的方法。你可以在[这篇博客文章](https://dioxuslabs.com/blog/templates-diffing/)中阅读更多关于 core 架构的信息。
* [core-types](https://github.com/DioxusLabs/dioxus/tree/main/packages/core-types): 核心类型 crate 包含 dioxus core 和热重载引擎中都使用的一些核心函数。
* [core-macro](https://github.com/DioxusLabs/dioxus/tree/main/packages/core-macro): 核心宏 crate 实现了 `derive(Props)` 和 `#[component]` 宏，用于派生组件构建。它还重新导出了 rsx 宏。
* [rsx](https://github.com/DioxusLabs/dioxus/tree/main/packages/rsx): 实现 RSX 宏的解析和扩展。解析器也用于热重载和 CLI 中的自动格式化。

### Web 工具 (Web utilities)

每个第一方 dioxus 渲染器都以 html 和 css 为目标。除了 blitz 之外，所有渲染器都在浏览器上下文中运行。Dioxus 在工作区中有一些工具，带有共享 trait 和 javascript 绑定，以帮助与浏览器交互：

* [interpreter](https://github.com/DioxusLabs/dioxus/tree/main/packages/interpreter): 解释器实现了来自 dioxus core 的 `WriteMutations` trait，以使用虚拟 DOM 生成的 diff 修改 DOM。解释器被 desktop、web 和 liveview 渲染器使用。它结合使用 [wasm-bindgen](https://rustwasm.github.io/wasm-bindgen) 和 [sledgehammer-bindgen](https://github.com/ealmloff/sledgehammer_bindgen) 与浏览器交互。
* [html](https://github.com/DioxusLabs/dioxus/tree/main/packages/html): 定义 html 特定的元素、事件和属性。元素和属性在 rsx 宏和热重载引擎中使用，以将 rust 标识符映射到 html 名称。html crate 中定义的事件是为每个平台定义的 trait。
* [html-internal-macro](https://github.com/DioxusLabs/dioxus/tree/main/packages/html-internal-macro): `html-internal-macro` crate 被 html crate 用于定义 html 元素和属性。
* [lazy-js-bundle](https://github.com/DioxusLabs/dioxus/tree/main/packages/lazy-js-bundle): 一个库，用于在构建时使用 bun 捆绑 typescript 文件（仅当内容更改时）。仅在文件更改时编译 typescript 并提交构建输出，使我们在将 dioxus 作为库添加时不需要安装 ts 编译器。
* [history](https://github.com/DioxusLabs/dioxus/tree/main/packages/history): `dioxus-history` crate 定义了每个渲染器必须提供的 history trait，以便与路由器一起使用。对于 web 渲染器，这应该调用 javascript history api。原生渲染器在内存中维护自己的历史堆栈。
* [document](https://github.com/DioxusLabs/dioxus/tree/main/packages/document): `dioxus-document` crate 定义了每个渲染器必须提供的 document trait，以便与 `eval` 和 `document::*` 组件一起使用。`eval` 从 rust 运行 javascript 代码，`document::*` 组件在 head 中创建 html 元素。

### 状态管理 (State Management)

* [generational-box](https://github.com/DioxusLabs/dioxus/tree/main/packages/generational-box): Generational Box 是 Dioxus 中所有 `Copy` 状态管理的核心。它分配一个在整个 dioxus 生态系统中使用的动态借用检查值的竞技场 (arena)。`GenerationalBox` 类型支持 dioxus signals 中的 `Signal`、`Memo` 和 `Resource`。它也用于 `dioxus-core` 中，使 `Closure` 和 `EventHandler` 类型变为 `Copy`。
* [signals](https://github.com/DioxusLabs/dioxus/tree/main/packages/signals): Signals 是 Dioxus 主要的面向用户的状态管理 crate。Signals 跟踪它们何时被读取和写入，并自动重新运行任何依赖于该 signal 的 `ReactiveContext`。
* [hooks](https://github.com/DioxusLabs/dioxus/tree/main/packages/hooks): Hooks 是 Dioxus 应用程序常用 hook 的集合。大多数 hook 是 `signals` crate 中新方法的薄封装，以便仅在创建组件时创建对象一次。

### 日志 (Logging)

* [logger](https://github.com/DioxusLabs/dioxus/tree/main/packages/logger): logger crate 为 Dioxus 应用程序提供了一个简单的日志接口，可在原生和 wasm 目标上工作。如果启用了 logging 特性，它会在 launch 函数中自动调用。

### 路由 (Routing)

* [router](https://github.com/DioxusLabs/dioxus/tree/main/packages/router): router crate 处理 Dioxus 应用程序中的路由。它使用渲染器提供的 history provider 来获取和修改 url。路由解析逻辑是使用 `dioxus-router-macro` crate 中定义的 `derive(Routable)` 宏派生的。
* [router-macro](https://github.com/DioxusLabs/dioxus/tree/main/packages/router-macro): router-macro crate 定义了 `derive(Routable)` 宏，用于从 url 路由枚举并将其显示为 url。

### 资源 (Assets)

* [manganis](https://github.com/DioxusLabs/dioxus/tree/main/packages/manganis/manganis): Manganis 是 dioxus 的资源系统。它使用宏将资源从 rust 代码注入到链接器中。每个资源都会获得一个唯一的哈希值用于缓存破坏。CLI 从链接器中提取资源并将它们捆绑到最终应用程序中。
* [manganis-macro](https://github.com/DioxusLabs/dioxus/tree/main/packages/manganis/manganis-macro): Manganis-macro 定义了 `asset!()` 宏，用于在 Dioxus 应用程序中包含资源。
* [manganis-core](https://github.com/DioxusLabs/dioxus/tree/main/packages/manganis/manganis-core): Manganis-core 包含传递给 `asset!()` 宏的所有选项的构建器，以及 asset 宏和 CLI 用于捆绑资源的链接部分。
* [const-serialize](https://github.com/DioxusLabs/dioxus/tree/main/packages/const-serialize): Const Serialize 定义了一个 trait，用于在编译时将 rust 类型序列化为跨平台格式。这用于在编译时序列化 manganis 中资源的选项。
* [const-serialize-macro](https://github.com/DioxusLabs/dioxus/tree/main/packages/const-serialize-macro): Const Serialize Macro 为可以使用 `const-serialize` crate 在编译时序列化的类型定义了一个派生宏。
* [cli-opt](https://github.com/DioxusLabs/dioxus/tree/main/packages/cli-opt): cli-opt 优化 manganis 生成的资源。

### 格式化 (Formatting)

* [autofmt](https://github.com/DioxusLabs/dioxus/tree/main/packages/autofmt): autofmt crate 查找并格式化 rust 项目中的所有 rsx 宏。它使用 `dioxus-rsx` crate 来解析 rsx。

### Lint 检查 (Linting)

* [check](https://github.com/DioxusLabs/dioxus/tree/main/packages/check): dioxus-check crate 分析 dioxus 代码以检查常见错误，例如在条件或循环中调用 hook。

### 翻译 (Translation)

* [rsx-rosetta](https://github.com/DioxusLabs/dioxus/tree/main/packages/rsx-rosetta): rsx-rosetta crate 将 html 翻译为 rsx。它使用 `dioxus-html` 中的元素定义将 html 元素和属性翻译为它们的 rust 名称，并使用 `rsx` crate 生成 rsx 宏。

### 热重载 (Hot Reloading)

* [rsx-hotreload](https://github.com/DioxusLabs/dioxus/tree/main/packages/rsx-hotreload): rsx-hotreload crate 处理构建之间 rsx 宏的差异比较，并为 CLI 创建热重载模板。
* [devtools](https://github.com/DioxusLabs/dioxus/tree/main/packages/devtools): devtools crate 包含每个渲染器需要集成的热重载前端。它从与 CLI 的 websocket 连接接收热重载消息。
* [devtools-types](https://github.com/DioxusLabs/dioxus/tree/main/packages/devtools-types): devtools-types crate 包含用于在 devtools 前端和 CLI 后端之间通信的类型。

### 命令行工具 (CLI)

* [cli](https://github.com/DioxusLabs/dioxus/tree/main/packages/cli): cli crate 包含 dioxus CLI。它集成了 check、autofmt、cli-opt 和 rsx-hotreload 来构建和服务 Dioxus 应用程序。
* [cli-config](https://github.com/DioxusLabs/dioxus/tree/main/packages/cli-config): cli-config crate 具有 CLI 在运行时提供给使用 CLI 构建的 crate 的共享类型。它被 `dioxus-desktop` 用于从 `Dioxus.toml` 文件设置标题，并被 `dioxus-fullstack` 用于设置 CLI 代理服务器的端口。
* [dx-wire-format](https://github.com/DioxusLabs/dioxus/tree/main/packages/dx-wire-format): dx-wire-format crate 具有 CLI 在 json 模式下发出的不稳定类型。这被 dioxus playground 使用。

### 扩展 (Extension)

* [extension](https://github.com/DioxusLabs/dioxus/tree/main/packages/extension): extension 文件夹包含 dioxus VSCode 扩展的源代码。它使用了许多与 CLI 相同的 crate，但打包成用于 VSCode 的 wasm+JS 包。

### 测试 (Testing)

* [playwright-tests](https://github.com/DioxusLabs/dioxus/tree/main/packages/playwright-tests): playwright-tests 文件夹包含 dioxus-web、dioxus-liveview 和 fullstack 的端到端测试。这些 crate 不会在 crates.io 上发布。
