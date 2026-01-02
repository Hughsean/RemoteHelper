# Dioxus 0.7 教程

在本教程中，我们将构建一个名为 HotDog 的小型应用程序——基本上就是狗狗版的 Tinder！这个应用程序将是学习构建 UI、添加状态和部署的绝佳方式。

到本教程结束时，你将发布你自己的 Web、桌面和移动应用程序，以及部署到 [Fly.io](http://fly.io/) 的后端。

![HotDog 应用截图](https://dioxuslabs.com/assets/dog_app_styled-dxh80ea3197f42fe9d1.webp)

我们将主要关注 Dioxus 的高级概念，而不会深入探讨特定 API 的细节。我们建议你自己尝试 API 或阅读[核心概念](https://dioxuslabs.com/learn/0.7/essentials/)和特定[指南](https://dioxuslabs.com/learn/0.7/guides/)以获取更多信息。

## 我们将学习什么？

本指南将涵盖“核心”Dioxus 功能，包括：

* [工具设置](#工具设置)
* [创建新应用](#创建新应用)
* [组件如何工作](#你的第一个组件)
* [使用 RSX 构建 UI](#使用-rsx-构建-ui)
* [样式和资源](#样式和资源)
* [添加状态](#添加状态)
* [获取数据](#获取数据)
* [添加后端](#添加后端)
* [集成数据库](#使用数据库)
* [应用路由](#路由和结构)
* [打包](#打包)
* [部署](#部署)
* [下一步](#下一步)

Dioxus 是一个功能非常全面的框架，因此我们鼓励你在完成本教程后构建自己的大型应用程序。

## 我们要构建什么？

HotDog 的功能相当简单：

* 浏览一系列可爱的狗狗照片
* 如果我们想将狗狗照片保存到收藏夹，就向右滑动
* 如果我们不想保存狗狗照片，就向左滑动
* 稍后查看我们保存的狗狗照片

---

## 工具设置

在开始之前，请确保你已按照[入门指南](https://dioxuslabs.com/learn/0.7/getting_started/)安装了所需的依赖项。我们将主要将 HotDog 开发为 Web 应用程序，但我们仍然建议为桌面和移动开发设置相关工具。

### 检查清单

我们已经在入门指南中涵盖了设置说明，但首先请验证你的设置：

* 已安装 Rust 和 Cargo
* 已安装 `wasm32-unknown-unknown` Rust 目标
* `dioxus-cli` 已安装且为最新版本
* 已安装系统特定的依赖项

参考 `dx doctor` 命令查看 `dx` 使用什么来构建你的应用。

### 所有命令

在继续之前，请确保你已安装并更新了 `dioxus-cli`。通过运行以下命令验证返回的版本是否与本指南匹配：

```bash
dx --version
```

你也可以运行 `dx help`，它将为你提供有用命令的列表以及如何使用 `dx` 的一些信息。

```text
Build, Bundle & Ship Dioxus Apps

Usage: dx [OPTIONS] <COMMAND>

Commands:
  build      Build the Dioxus project and all of its assets
  translate  Translate a source file into Dioxus code
  serve      Build, watch & serve the Dioxus project and all of its assets
  new        Create a new project for Dioxus
  init       Init a new project for Dioxus in the current directory
  clean      Clean output artifacts
  bundle     Bundle the Dioxus app into a shippable object
  fmt        Automatically format RSX
  check      Check the project for any issues
  run        Run the project without any hotreloading
  config     Dioxus config file controls
  help       Print this message or the help of the given subcommand(s)
```

如果 `dx` 安装正确，那么你就准备好继续了！

---

## 创建新应用

你可以通过运行以下命令并按照提示操作来创建一个新的 Dioxus 项目：

```bash
dx new hot_dog
```

你需要选择一个模板来开始。

* **Bare-bones**: 一个非常简单的设置，只有一个 `main.rs` 和一个 `assets` 文件夹。
* **Jumpstart**: 一个带有组件、视图和建议结构的脚手架应用。
* **Workspace**: 一个完整的 cargo 工作区设置，每个平台有不同的 crate。

我们将为 HotDog 使用 **bare-bones** 模板，因为我们的应用将非常简单。

* 当被问及是否要创建全栈网站时，选择 "false"。
* 对于路由器选择 "false"，尽管我们最终会将路由器添加到应用中。
* 对于 TailwindCSS 选择 "true"。
* 对于 LLM 提示选择 "false"。
* 选择 "Web" 作为默认平台。

> 📣 你不需要 `dx new` 来创建新的 Dioxus 应用！Dioxus 应用是 Rust 项目，也可以使用 cargo 等工具构建。

### 运行项目

项目生成后，你可以使用以下命令启动它：

```bash
cd hot_dog
dx serve
```

这将启动 cargo 构建并启动一个 Web 服务器来服务你的应用。如果你访问 "serve" 地址（在本例中为 `http://127.0.0.1:8080`），那么你将在浏览器中看到加载屏幕。

应用加载后，你应该会看到默认的 Dioxus 模板应用。

恭喜！你拥有了你的第一个 Dioxus 应用。

### 应用结构

在编辑器中打开应用并查看其结构：

```text
├── Cargo.lock
├── Cargo.toml
├── Dioxus.toml
├── README.md
├── assets
│   ├── favicon.ico
│   ├── header.svg
│   └── main.css
└── src
    └── main.rs
```

所有 Rust 应用都由根目录下的 `Cargo.toml` 和位于 `src` 文件夹中的 `main.rs` 文件组成。我们的 CLI `dx` 为我们预填充了这些文件，其中包含 `dioxus` 依赖项和一些启动代码，以便我们快速开始构建。

Dioxus 中的资源可以放置在项目中的任何位置，但我们建议将它们留在 `assets` 文件夹中。

#### Cargo.toml

`Cargo.toml` 概述了我们应用的依赖项并指定了编译器设置。

```toml
[dependencies]
dioxus = { version = "0.7.0" }

[features]
default = ["web"]
web = ["dioxus/web"]
desktop = ["dioxus/desktop"]
mobile = ["dioxus/mobile"]
```

#### Dioxus.toml

`Dioxus.toml` 文件包含用于打包和部署应用的 Dioxus 特定配置。我们暂时不需要为我们的应用配置 `Dioxus.toml`。

#### Assets 文件夹

要在 Dioxus 应用中包含资源，你需要使用 `asset!()` 宏。你可以从应用文件树中的任何位置包含资源，但我们建议使用预生成的 `assets` 文件夹。

#### tailwind.css

Dioxus 内置支持 TailwindCSS！在服务和构建应用时，如果 `dx` 在应用根目录检测到 `tailwind.css`，它会自动运行 TailwindCSS CLI。

#### main.rs

最后是 `main.rs`。`main.rs` 文件是应用的入口点，包含 `fn main` 函数。

```rust
use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}
```

`launch` 函数根据 `dioxus` 上启用的功能（web/desktop/mobile）调用特定于平台的 `launch` 函数。`launch` 接受一个根组件，通常称为 `App`。

### 重置为基础

bare-bones 模板为我们的应用提供了基本的启动代码。但是，我们要真正从头开始，所以我们将擦除 `Hero` 组件并将 `App` 组件清空到最基础的状态：

```rust
use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! { "HotDog!" }
}
```

---

## 你的第一个组件

现在我们已经初始化了 HotDog 应用，我们可以开始构建它的组件了。

### 什么是组件？

在 Dioxus 中，应用由称为组件的独立函数组成，这些函数接收一些属性（Properties）并渲染一个元素（Element）：

```rust
fn DogApp(props: DogAppProps) -> Element {
    todo!()
}
```

### 组件属性

所有组件都接受一个对象，该对象概述了组件可以接受哪些参数。Dioxus 中的所有 `Props` 结构体都需要派生 `Properties` trait，这需要 `Clone` 和 `PartialEq`：

```rust
#[derive(Props, PartialEq, Clone)]
struct DogAppProps {
    breed: String,
}
```

Dioxus 提供了 `#[component]` 宏来简化组件的定义。此宏将带注释函数的参数转换为隐藏的伴随结构体。

```rust
#[component]
fn DogApp(breed: String) -> Element {
    todo!()
}
```

### 属性是不可变的

就像 React 一样，Dioxus 组件通过调用函数组件来渲染。在每次渲染时，Dioxus 都会对组件的 props 进行 `.clone()`。这确保你不会意外修改 props，从而导致难以追踪的状态管理问题。

```rust
#[component]
fn DogApp(breed: String) -> Element {
    tracing::info!("Rendered with breed: {breed}");

    todo!()
}
```

### 组件函数被多次调用

就像 React 一样，Dioxus 会在其生命周期内多次调用你的组件函数。这称为重新渲染。在 Dioxus 中，重新渲染非常廉价（比 React 便宜得多！）。

当 Dioxus 重新渲染你的组件时，它会将上次渲染的 `Element` 与当前渲染的 `Element` 进行比较。

Dioxus 仅在两种情况下重新渲染你的组件：

* 当 `Props` 发生变化时（由 `PartialEq` 确定）
* 当像 `signal.set()` 或 `signal.write()` 这样的函数调用 `Scope.needs_update()` 时

### 组合组件

在 Dioxus 中，组件组合在一起创建应用。每个组件将持有自己的状态并处理自己的更新。

要组合组件，我们将使用 `rsx! {}` 宏来定义我们应用的结构。

```rust
#[component]
fn App() -> Element {
    rsx! {
        Header {}
        DogApp { breed: "corgi" }
        Footer {}
    }
}
```

---

## 使用 RSX 构建 UI

Dioxus 是一个声明式框架。这意味着我们只需使用 RSX 声明我们希望 UI 看起来是什么样子，而不是告诉 Dioxus 做什么。

```rust
// 定义一个渲染带有文本 "Hello, world!" 的 div 的组件
fn App() -> Element {
    rsx! {
        div { "Hello, world!" }
    }
}
```

### 使用热重载编辑 RSX

当使用 `dx serve` 时，每当你编辑并保存文件时，你的应用的 RSX 会自动热重载。你可以编辑 RSX 结构、添加新元素和样式化标记，而无需完全重建。

每当你编辑 Rust 代码时，`dx` 将自动强制“完全重建”你的应用。

### RSX 只是 HTML

Dioxus 提供了 `rsx! {}` 宏来在你的应用中组装 `Element`。`rsx! {}` 宏主要讲 HTML：Web、桌面和移动 Dioxus 第一方渲染器都使用 HTML 和 CSS 作为布局和样式技术。

RSX 语法是 Rust 的一种“严格”形式，使用 Rust 的 `Struct` 语法来组装元素：

```rust
rsx! {
    div {
        class: "bg-red-100"
    }
}
```

RSX 中的元素与 Rust 结构体语法略有不同：它们也可以包含放置在最后一个属性之后的子结构体。

```rust
rsx! {
    div { class: "bg-red-100",
        button {
            onclick: move |_| info!("Clicked"),
            "Click me!"
        }
    }
}
```

此外，RSX 中所有带引号的字符串都自动暗示 `format!()`，因此你可以在标记之外定义变量并在字符串中使用它：

```rust
rsx! {
    div { "Breed: {breed}" }
}
```

任何可以渲染为 String 的表达式都可以直接包含在 RSX 中。RSX 也接受 `Option<Element>` 和 Elements 迭代器：

```rust
rsx! {
    // 任何实现了 `Display` 的东西
    {"Something"}

    // 可选项
    {show_title.then(|| rsx! { "title!" } )}

    // 以及迭代器
    ul {
        {(0..5).map(|i| rsx! { li { "{i}" } })}
    }
}
```

Dioxus 为这些常见情况提供了两种语法糖：`for` 循环和 `if` 链。

```rust
rsx! {
    if show_title {
        "title!"
    }

    ul {
        for item in 0..5 {
            li { "{item}" }
        }
    }
}
```

对于列表，Dioxus 使用 `key` 属性来确保在渲染之间比较正确的元素。

```rust
rsx! {
    for user in users {
        div {
            key: "{user.id}",
            "{user.name}"
        }
    }
}
```

### 向 HotDog 应用添加 UI

让我们向我们的应用添加一个基本的 UI。我们将添加一个标题、一个用于狗狗照片的主体图像和一些基本按钮。

```rust
#[component]
fn App() -> Element {
    rsx! {
        div { id: "title",
            h1 { "HotDog! 🌭" }
        }
        div { id: "dogview",
            img { src: "https://images.dog.ceo/breeds/pitbull/dog-3981540_1280.jpg" }
        }
        div { id: "buttons",
            button { id: "skip", "skip" }
            button { id: "save", "save!" }
        }
    }
}
```

---

## 样式和资源

### Dioxus 使用 CSS 进行样式设计

Dioxus 应用使用 HTML 和 CSS 作为核心标记和样式技术。

### 使用 asset!() 添加 CSS 文件

bare-bones 模板已经在 `assets` 文件夹中包含了一个基础的 `main.css`。

要将 CSS 包含在我们的应用中，我们可以使用 `asset!()` 宏。此宏确保资源将包含在最终的应用包中。

```rust
static CSS: Asset = asset!("/assets/main.css");
```

我们还需要使用 `document::Stylesheet` 组件将资源加载到我们的应用中。

```rust
fn App() -> Element {
    rsx! {
        document::Stylesheet { href: CSS }
    }
}
```

### 热重载

Dioxus 中的所有资源都参与热重载。尝试编辑应用的 `main.css` 并观察更改实时传播。

### 包含图像

在 Dioxus 中，你可以通过两种方式包含图像：

* 使用 URL 动态包含
* 使用 `asset!()` 宏静态包含

对于静态图像，你可以使用与包含应用 CSS 相同的 `asset!()` 宏。

```rust
static ICON: Asset = asset!("/assets/icon.png");

rsx! {
    img { src: ICON }
}
```

### 优化

默认情况下，`asset!()` 宏将轻微优化 CSS、JavaScript、JSON 和图像。资源的名称也将被修改以包含内容哈希。

你可以使用可选的 `Options` 结构体进一步优化资源。例如，`dx` 可以自动将 `.png` 图像转换为更优化的 `.avif` 格式：

```rust
// 输出 icon-j1238jd2.avif
asset!("/assets/icon.png", AssetOptions::image().with_avif());
```

### 最终的 CSS

我们可以使用 `dx` 的资源热重载系统和我们的 CSS 知识来创建一个漂亮的应用。

（此处省略了详细的 CSS 代码，请参考原始教程或根据需要添加）

---

## 添加状态

现在我们的 HotDog 应用已经搭建好并有了样式，我们可以添加一些交互元素了。

### 封装状态

在深入之前，让我们将应用拆分为两部分：`Title` 和 `DogView`。这将帮助我们组织应用并将 `DogView` 状态与 `Title` 状态分离。

```rust
#[component]
fn App() -> Element {
    rsx! {
        document::Stylesheet { href: CSS }
        Title {}
        DogView {}
    }
}

#[component]
fn Title() -> Element {
    rsx! {
        div { id: "title",
            h1 { "HotDog! 🌭" }
        }
    }
}

#[component]
fn DogView() -> Element {
    rsx! {
        div { id: "dogview",
            img { src: "https://images.dog.ceo/breeds/pitbull/dog-3981540_1280.jpg" }
        }
        div { id: "buttons",
            button { id: "skip", "skip" }
            button { id: "save", "save!" }
        }
    }
}
```

### 事件处理程序

在 `DogView` 组件中，我们希望将操作附加到按钮的点击上。我们可以使用 `EventHandler` 来监听 `click` 事件。

```rust
#[component]
fn DogView() -> Element {
    let skip = move |evt| {};
    let save = move |evt| {};

    rsx! {
        // ...
        div { id: "buttons",
            button { onclick: skip, id: "skip",  "skip" }
            button { onclick: save, id: "save",  "save!" }
        }
    }
}
```

### 使用 use_hook 的状态

为了在组件中存储状态，Dioxus 提供了 `use_hook` 函数。这使得裸 Rust 函数可以存储和加载状态，而无需使用额外的结构体。

```rust
#[component]
fn DogView() -> Element {
    let img_src = use_hook(|| "https://images.dog.ceo/breeds/pitbull/dog-3981540_1280.jpg");

    // ..

    rsx! {
        div { id: "dogview",
            img { src: "{img_src}" }
        }
        // ..
    }
}
```

### 信号和 use_signal

虽然 `use_hook` 可以存储任何实现了 `Clone` 的值，但你经常需要一种更强大的状态管理形式。Dioxus 内置了信号（Signals）。

`Signal` 是普通 Rust 值的包装类型，它跟踪读取和写入，使你的应用栩栩如生。我们强烈建议使用 `use_signal` 钩子。

每当信号的值发生变化时，其包含的“响应式作用域”将被“标记为脏”并重新运行。默认情况下，Dioxus 组件是响应式作用域，因此，每当信号值发生变化时，组件都会重新渲染。

### 使用 Context 的全局状态

Dioxus 提供了两种机制来管理整个应用的状态：`Context` 和 `GlobalSignal`。

`Context` API 使得父组件可以与子组件共享状态，而无需显式声明额外的属性字段。

```rust
// 创建一个新的包装类型
#[derive(Clone)]
struct TitleState(String);

fn App() -> Element {
    // 提供该类型作为 Context
    use_context_provider(|| TitleState("HotDog".to_string()));
    rsx! {
        Title {}
    }
}

fn Title() -> Element {
    // 消费该类型作为 Context
    let title = use_context::<TitleState>();
    rsx! {
        h1 { "{title.0}" }
    }
}
```

### 全局信号

偶尔你可能想要一个简单的全局值。这就是 `GlobalSignal` 的用武之地。

```rust
static SONG: GlobalSignal<String> = Signal::global(|| "Drift Away".to_string());
```

然后从任何地方读取和写入它：

```rust
#[component]
fn Player() -> Element {
    rsx! {
        h3 { "Now playing {SONG}" }
        button {
            onclick: move |_| *SONG.write() = "Vienna".to_string(),
            "Shuffle"
        }
    }
}
```

---

## 获取数据

### 添加依赖项

Dioxus 没有提供任何内置的数据获取工具。对于本教程，我们将从头开始实现数据获取。

首先，我们需要向应用添加两个新依赖项：`serde` 和 `reqwest`。

```bash
cargo add reqwest --features json
cargo add serde --features derive
```

### 定义响应类型

我们将使用 [dog.ceo/dog-api](https://dog.ceo/dog-api/) 来获取狗狗的图片。

```rust
#[derive(serde::Deserialize)]
struct DogApi {
    message: String,
}
```

### 使用 reqwest 和 async

Dioxus 对异步 Rust 有很好的支持。我们可以简单地将 `onclick` 处理程序转换为 `async`，然后在 future 解析后设置 `img_src`。

```rust
#[component]
fn DogView() -> Element {
    let mut img_src = use_signal(|| "".to_string());

    let save = move |_| async move {
        let response = reqwest::get("https://dog.ceo/api/breeds/image/random")
            .await
            .unwrap()
            .json::<DogApi>()
            .await
            .unwrap();

        img_src.set(response.message);
    };

    // ..

    rsx! {
        div { id: "dogview",
            img { src: "{img_src}" }
        }
        div { id: "buttons",
            // ..
            button { onclick: save, id: "save", "save!" }
        }
    }
}
```

### 使用 use_resource 获取数据

在 Dioxus 中，Resources 是其值依赖于某些异步工作完成的状态。`use_resource` 钩子提供了一个 `Resource` 对象，其中包含启动、停止、暂停和修改异步状态的有用方法。

让我们更改组件以使用资源：

```rust
#[component]
fn DogView() -> Element {
    let mut img_src = use_resource(|| async move {
        reqwest::get("https://dog.ceo/api/breeds/image/random")
            .await
            .unwrap()
            .json::<DogApi>()
            .await
            .unwrap()
            .message
    });

    rsx! {
        div { id: "dogview",
            img { src: img_src.cloned().unwrap_or_default() }
        }
        div { id: "buttons",
            button { onclick: move |_| img_src.restart(), id: "skip", "skip" }
            button { onclick: move |_| img_src.restart(), id: "save", "save!" }
        }
    }
}
```

---

## 添加后端

Dioxus 是一个全栈框架，使你能够无缝地构建前端和后端。

### 启用全栈

在我们可以开始使用服务器函数之前，我们需要在 `Cargo.toml` 中启用 Dioxus 的 "fullstack" 功能。

```toml
[dependencies]
dioxus = { version = "0.7.0", features = ["fullstack"] }
```

我们还需要将 "server" 功能添加到我们应用的功能中，并删除默认的 web 目标。

```toml
[features]
default = [] # <----- 删除默认的 web 目标
web = ["dioxus/web"]
desktop = ["dioxus/desktop"]
mobile = ["dioxus/mobile"]
server = ["dioxus/server"] # <----- 添加此额外目标
```

现在，你需要使用手动平台运行 `dx serve --web`，而不是运行 `dx serve`。

### 服务器函数：内联 RPC 系统

Dioxus 集成了 `axum` crate，为你的应用提供简单的内联通信系统。服务器函数是用过程宏（如 post/get）注释的 `async` 函数。

```rust
#[post("/api/save_dog")]
async fn save_dog(image: String) -> Result<()> {
    Ok(())
}
```

在客户端，服务器函数扩展为 `reqwest` 调用。在服务器上，服务器函数扩展为 `axum` 处理程序。

### 客户端/服务器分离

当 Dioxus 构建你的全栈应用时，它实际上创建了两个独立的应用程序：服务器和客户端。

* 客户端使用 `--features web` 构建
* 服务器使用 `--features server` 构建

我们建议将仅限服务器的代码放置在为 `"server"` 功能配置的模块中。

```rust
// ✅ 此模块中的代码只能在服务器上访问
#[cfg(feature = "server")]
mod server_utils {
    pub static DB_PASSWORD: &str = "1234";
}
```

### 我们的 HotDog 服务器函数

让我们创建一个新的服务器函数，将我们最喜欢的狗狗保存到名为 `dogs.txt` 的文件中。

```rust
// 在我们的服务器上公开一个 `save_dog` 端点，该端点接受一个 "image" 参数
#[post("/api/save_dog")]
async fn save_dog(image: String) -> Result<()> {
    use std::io::Write;

    // 以仅追加模式打开 `dogs.txt` 文件，如果不存在则创建它
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .create(true)
        .open("dogs.txt")
        .unwrap();

    // 然后将带有图像 url 的换行符写入其中
    file.write_fmt(format_args!("{image}\n"));

    Ok(())
}
```

#### 调用服务器函数

现在，在我们的客户端代码中，我们可以实际调用服务器函数。

```rust
fn DogView() -> Element {
    let mut img_src = use_resource(snipped!());

    // ...
    rsx! {
        // ...
        div { id: "buttons",
            // ...
            button {
                id: "save",
                onclick: move |_| async move {
                    let current = img_src.cloned().unwrap();
                    img_src.restart();
                    _ = save_dog(current).await;
                },

                "save!"
            }
        }
    }
}
```

---

## 使用数据库

### 选择数据库

对于 HotDog，我们将使用 Sqlite。

### 向 HotDog 添加数据库操作

为了向 HotDog 添加 sqlite 功能，我们将引入 `rusqlite` crate。注意 `rusqlite` 仅用于在服务器上编译，因此我们将在 `Cargo.toml` 中将其功能门控在 `"server"` 功能后面。

```toml
[dependencies]
rusqlite = { version = "0.32.1", optional = true } # <--- 添加 rusqlite

[features]
server = ["dioxus/server", "dep:rusqlite"] # <---- 添加 dep:rusqlite
```

为了连接到我们的数据库，我们将使用 `rusqlite::Connection`。

```rust
// 数据库仅对服务器代码可用
#[cfg(feature = "server")]
thread_local! {
    pub static DB: rusqlite::Connection = {
        // 从持久化的 "hotdog.db" 文件打开数据库
        let conn = rusqlite::Connection::open("hotdog.db").expect("Failed to open database");

        // 如果 "dogs" 表不存在，则创建它
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS dogs (
                id INTEGER PRIMARY KEY,
                url TEXT NOT NULL
            );",
        ).unwrap();
        
        // 返回连接
        conn
    };
}
```

现在，在我们的 `save_dog` 服务器函数中，我们可以使用 SQL 将值插入数据库：

```rust
#[server]
async fn save_dog(image: String) -> Result<()> {
    DB.with(|f| f.execute("INSERT INTO dogs (url) VALUES (?1)", &[&image]))?;
    Ok(())
}
```

---

## 路由和结构

到目前为止，我们的应用只有一个页面。让我们改变它！

### 组织我们的项目

我们通常建议将组件、模型和后端功能拆分到不同的文件中。

```text
├── Cargo.toml
├── assets
│   └── main.css
└── src
    ├── backend.rs
    ├── components
    │   ├── favorites.rs
    │   ├── mod.rs
    │   ├── nav.rs
    │   └── view.rs
    └── main.rs
```

### 创建路由

Dioxus 提供了一个与 Web、桌面和移动原生集成的第一方路由器。首先，我们需要在 `Cargo.toml` 文件中添加 "Router" 功能：

```toml
[dependencies]
dioxus = { version = "0.7.0", features = ["fullstack", "router"] } # <----- 添加 "router"
```

接下来，Dioxus 路由器定义为带有 `Routable` 派生属性的枚举：

```rust
#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[route("/")]
    DogView,
}
```

### 渲染路由

现在我们需要渲染它。让我们更改 `app` 组件以渲染 `Router {}` 组件而不是 `DogView`。

```rust
fn app() -> Element {
    rsx! {
        document::Stylesheet { href: asset!("/assets/main.css") }

        // 📣 删除 Title 和 DogView 并将其替换为 Router 组件。
        Router::<Route> {}
    }
}
```

### 使用布局渲染 NavBar

在我们的 `src/components/nav.rs` 文件中，我们将添加回我们的 Title 代码，但将其重命名为 NavBar 并使用两个新项目进行修改：`Link {}` 和 `Outlet` 组件。

```rust
use crate::Route;
use dioxus::prelude::*;

#[component]
pub fn NavBar() -> Element {
    rsx! {
        div { id: "title",
            Link { to: Route::DogView,
                h1 { "🌭 HotDog! " }
            }
        }
        Outlet::<Route> {}
    }
}
```

要实际将 NavBar 组件添加到我们的应用中，我们需要使用 `#[layout]` 属性更新我们的 `Route` 枚举。

```rust
#[derive(Routable, PartialEq, Clone)]
enum Route {
    #[layout(NavBar)] // <---- 添加 #[layout] 属性
    #[route("/")]
    DogView,
}
```

### 添加收藏夹路由

让我们确保在 `Route` 枚举中添加一个新变体：

```rust
#[derive(Routable, PartialEq, Clone)]
enum Route {
    #[layout(NavBar)]
    #[route("/")]
    DogView,

    #[route("/favorites")]
    Favorites, // <------ 添加此新变体
}
```

### 我们的收藏夹页面

最后，我们可以构建我们的收藏夹页面。让我们添加一个新的 `list_dogs` 服务器函数，该函数获取最近保存的 10 张狗狗照片：

```rust
// 查询数据库并返回最后 10 只狗及其 url
#[server]
pub async fn list_dogs() -> Result<Vec<(usize, String)>, ServerFnError> {
    let dogs = DB.with(|f| {
        f.prepare("SELECT id, url FROM dogs ORDER BY id DESC LIMIT 10")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    });

    Ok(dogs)
}
```

---

## 打包

### 在 iOS 上测试

只需运行 `dx serve --ios`，你的应用就应该在 iOS 模拟器中加载。

### 在 Android 上测试

如果一切顺利，我们可以简单地服务，我们的应用应该会出现在我们的 Android 模拟器中。

```bash
dx serve --android
```

### 在桌面上测试

我们可以使用 `dx serve --desktop` 将我们的应用作为桌面应用服务。

### 为 Web 打包

要开始，让我们构建我们应用的 Web 版本。

```bash
dx bundle --web
```

`dx` 构建了一个包含我们的资源、index.html 和各种 JavaScript 片段的 `public` 文件夹。

### 为桌面和移动打包

要打包桌面和移动应用以进行部署，我们将再次使用 `dx bundle`。

```bash
dx bundle --desktop \
    --package-types "macos" \
    --package-types "dmg"
```

### 自定义你的包

要配置我们的包，我们将使用 `Dioxus.toml` 并修改 bundle 部分。

```toml
[application]
name = "docsite"

[bundle]
identifier = "com.dioxuslabs"
publisher = "DioxusLabs"
icon = ["assets/icon.png"]
```

---

## 部署

### Dioxus Deploy

目前，Dioxus 不提供自己的部署平台。

### 部署要求

Dioxus Web 应用构建为客户端包和服务器可执行文件。通常，任何公开简单容器的部署提供商都足以用于 Dioxus 全栈 Web 应用程序。

### 构建 Dockerfile

对于 Rust 应用，通常没有预构建的“包”作为目标。在这些情况下，我们需要编写一个简单的 Dockerfile 来编译和启动我们的应用。

```dockerfile
FROM rust:1 AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .

# 安装 dx
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall dioxus-cli --root /.cargo -y --force
ENV PATH="/.cargo/bin:$PATH"

# 创建最终包文件夹
RUN dx bundle --web --release

FROM chef AS runtime
COPY --from=builder /app/target/dx/hot_dog/release/web/ /usr/local/app

# 设置端口
ENV PORT=8080
ENV IP=0.0.0.0
EXPOSE 8080

WORKDIR /usr/local/app
ENTRYPOINT [ "/usr/local/app/server" ]
```

### 部署到 Fly

要开始使用 Fly，我们需要完成注册流程。我们将添加上面的 dockerfile 以及 dockerignore。

让我们调用 `fly launch`，它将自动初始化我们的 `fly.toml`。

### 全栈桌面和移动

现在我们的后端已上线，我们可以将 API 连接到我们的原生应用。默认情况下，Dioxus 不知道在哪里可以找到你的 API，因此你需要通过调用 `server_fn::client::set_server_url` 手动指定 URL。

```rust
fn main() {
    #[cfg(not(feature = "server"))]
    server_fn::client::set_server_url("https://hot-dog.fly.dev");

    dioxus::launch(App);
}
```

---

## 下一步

我们的应用还没有完成，但本指南已经很长了！

还有很多额外的事情要做：

* 添加用户、登录和身份验证。
* 使用 Cloudflare 等工具保护我们的网站免受 DDOS 攻击。
* 添加更多功能
* 营销并与朋友分享！

### 常见问题

* **Dioxus 快吗？** Dioxus 非常快。Dioxus 围绕极其高性能的 VirtualDom 构建。
* **Rust 太难了吗？** Rust 是一门出了名难学的语言，但它非常强大。Dioxus 旨在利用 Rust “简单”的部分。
* **Dioxus 支持 "xyz" 吗？**
  * TailwindCSS: 是
  * 原生组件: 是
  * AI: 是
* **为什么用 RSX 而不是 HTML？** RSX 获得令牌着色和代码折叠，无需额外工具；RSX 输入更快，因为花括号自动闭合。

恭喜你完成了我们的 HotDog 教程！希望这不是我们旅程的终点，而是一个大胆的新起点。
