# Dioxus 0.7 教程 (Tutorial)

这份文档汇编了 Dioxus 0.7 官方教程的核心内容，旨在作为 AI Agent 的知识库。内容涵盖了从环境设置到全栈应用部署的完整流程。

## 1. 概览 (Overview)

Dioxus 是一个用于构建 Web、桌面和移动应用的全栈 Rust 框架。本教程将指导你构建一个名为 "HotDog" 的应用（类似于 Tinder，但用于浏览狗狗图片），涵盖以下核心功能：

- 浏览狗狗图片流
- 喜欢（保存）或跳过图片
- 查看已保存的收藏列表
- 全栈后端集成和数据库存储

## 2. 工具设置 (Tooling Setup)

### 前置要求

- Rust 和 Cargo 已安装
- `wasm32-unknown-unknown` target 已安装 (`rustup target add wasm32-unknown-unknown`)
- `dioxus-cli` 已安装 (`cargo install dioxus-cli`)

### 常用命令 (`dx`)

- `dx serve`: 构建、监听文件变化并服务应用（支持热重载）。
- `dx build`: 构建项目。
- `dx bundle`: 将应用打包为可发布格式。
- `dx new`: 创建新项目。
- `dx doctor`: 检查环境配置。

## 3. 创建新应用 (Creating a new app)

### 创建项目

```bash
dx new hot_dog
```

选择 "Bare-bones" 模板，平台选择 "Web"。

### 运行项目

```bash
cd hot_dog
dx serve
```

默认地址: `http://127.0.0.1:8080`

### 项目结构

- `Cargo.toml`: 依赖管理。Dioxus 应用通常包含 `dioxus` 依赖。
- `Dioxus.toml`: Dioxus CLI 配置文件。
- `assets/`: 静态资源文件夹（CSS, 图片等）。
- `src/main.rs`: 应用入口。

### 入口代码 (`src/main.rs`)

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

## 4. 你的第一个组件 (Your First Component)

### 组件定义

组件是接受 Props 并返回 `Element` 的函数。使用 `#[component]` 宏来简化定义。

```rust
#[component]
fn DogApp(breed: String) -> Element {
    rsx! {
        "Breed: {breed}"
    }
}
```

### Props (属性)

- Props 是不可变的。
- 每次渲染都会被克隆。
- 自定义 Props 结构体需要派生 `Props`, `PartialEq`, `Clone`。

### 组件组合

在 `rsx!` 中像 HTML 标签一样使用组件：

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

## 5. 使用 RSX 构建 UI (Creating UI with RSX)

### RSX 语法

RSX 类似于 HTML，但是是 Rust 宏。它支持 Rust 表达式和控制流。

```rust
rsx! {
    div { class: "bg-red-100",
        button {
            onclick: move |_| println!("Clicked"),
            "Click me!"
        }
    }
}
```

### 动态内容

- 字符串插值: `"Breed: {breed}"`
- 迭代器:

```rust
ul {
    for item in 0..5 {
        li { "{item}" }
    }
}
```

- 条件渲染:

```rust
if show_title {
    "title!"
}
```

- **注意**: 列表项需要唯一的 `key` 属性以优化 diff 算法。

## 6. 样式和资源 (Styling and Assets)

### 引入 CSS

使用 `asset!()` 宏引入 CSS 文件，并通过 `document::Stylesheet` 加载。

```rust
static CSS: Asset = asset!("/assets/main.css");

fn App() -> Element {
    rsx! {
        document::Stylesheet { href: CSS }
    }
}
```

### 引入图片

- 动态 URL: `img { src: "https://..." }`
- 静态资源:

```rust
static ICON: Asset = asset!("/assets/icon.png");
rsx! { img { src: ICON } }
```

### TailwindCSS

Dioxus 内置支持 TailwindCSS。如果在项目根目录检测到 `tailwind.css`，`dx` 会自动运行 Tailwind CLI。

## 7. 添加状态 (Adding State)

### 信号 (Signals)

使用 `use_signal` 创建响应式状态。

```rust
#[component]
fn DogView() -> Element {
    let mut img_src = use_signal(|| "https://dog.ceo/...".to_string());

    rsx! {
        img { src: "{img_src}" }
        button {
            onclick: move |_| img_src.set("new_url".to_string()),
            "Change"
        }
    }
}
```

### 上下文 (Context)

用于跨组件共享状态。

- 提供: `use_context_provider(|| MyState { ... })`
- 消费: `let state = use_context::<MyState>()`

### 事件处理 (Event Handlers)

类似于 HTML 属性，但接受闭包。

```rust
button { onclick: move |evt| println!("Clicked"), "Save" }
```

## 8. 获取数据 (Fetching Data)

### 依赖

添加 `reqwest` (HTTP 客户端) 和 `serde` (序列化)。

### 使用 `use_resource`

`use_resource` 用于管理异步任务，支持 Suspense 和服务端渲染。

```rust
#[derive(serde::Deserialize)]
struct DogApi { message: String }

#[component]
fn DogView() -> Element {
    // 创建一个资源，当组件挂载时自动运行
    let mut img_src = use_resource(|| async move {
        reqwest::get("https://dog.ceo/api/breeds/image/random")
            .await.unwrap()
            .json::<DogApi>()
            .await.unwrap()
            .message
    });

    rsx! {
        // 处理加载状态和数据
        match &*img_src.read_unchecked() {
            Some(src) => rsx! { img { src: "{src}" } },
            None => rsx! { "Loading..." }
        }
        button { onclick: move |_| img_src.restart(), "Next" }
    }
}
```

## 9. 添加后端 (Add a Backend)

### 启用 Fullstack

在 `Cargo.toml` 中启用 `fullstack` 特性，并添加 `server` feature。

### Server Functions

使用 `#[server]` 宏定义的函数将在服务器上执行，客户端可以通过网络调用它们（RPC）。

```rust
#[server]
async fn save_dog(image: String) -> Result<(), ServerFnError> {
    // 这段代码只在服务器运行
    println!("Saving dog: {}", image);
    Ok(())
}
```

### 客户端/服务器代码分离

使用 `#[cfg(feature = "server")]` 标记仅服务器代码。

```rust
#[cfg(feature = "server")]
mod server_utils {
    pub static DB_URL: &str = "sqlite://hotdog.db";
}
```

## 10. 使用数据库 (Working with Databases)

### 集成 SQLite

添加 `rusqlite` 依赖 (通常标记为 `optional = true` 并在 `server` feature 中启用)。

### 数据库连接

由于 `rusqlite::Connection` 不是线程安全的，建议使用 `thread_local!`。

```rust
#[cfg(feature = "server")]
thread_local! {
    pub static DB: rusqlite::Connection = {
        let conn = rusqlite::Connection::open("hotdog.db").unwrap();
        conn.execute("CREATE TABLE IF NOT EXISTS dogs ...", []).unwrap();
        conn
    };
}

#[server]
async fn list_dogs() -> Result<Vec<String>, ServerFnError> {
    let dogs = DB.with(|conn| {
        // 执行查询...
        Ok(vec![])
    });
    Ok(dogs?)
}
```

## 11. 路由和结构 (Routing and Structure)

### 设置路由

添加 `router` feature。定义路由枚举并派生 `Routable`。

```rust
#[derive(Routable, Clone, PartialEq)]
enum Route {
    #[layout(NavBar)] // 使用布局组件
        #[route("/")]
        DogView,
        #[route("/favorites")]
        Favorites,
    #[end_layout]
    #[route("/:..segments")] // 404 处理
    PageNotFound { segments: Vec<String> },
}

fn App() -> Element {
    rsx! { Router::<Route> {} }
}
```

### 导航

使用 `Link` 组件或 `Outlet`。

```rust
#[component]
fn NavBar() -> Element {
    rsx! {
        nav {
            Link { to: Route::DogView, "Home" }
            Link { to: Route::Favorites, "Favorites" }
        }
        Outlet::<Route> {} // 渲染当前路由的内容
    }
}
```

## 12. 打包 (Bundling)

### Web 打包

```bash
dx bundle --web --release
```

生成 `dist` 或 `public` 文件夹，包含优化后的 WASM、JS 和静态资源。

### 桌面/移动打包

```bash
dx bundle --desktop
dx bundle --mobile
```

支持多种格式：`.app`, `.dmg`, `.exe`, `.deb`, `.apk` 等。

### 优化

`dx` 会自动优化资源（如将图片转换为 avif，压缩 CSS/JS）。

## 13. 部署 (Deploying)

### Docker 部署 (Fullstack)

对于全栈应用，需要部署服务端二进制文件和客户端资源。推荐使用多阶段 Docker 构建：

1. **Chef**: 缓存依赖。
2. **Builder**: 构建服务端二进制文件和客户端资源 (`dx bundle --web`).
3. **Runtime**: 运行应用 (设置 `IP=0.0.0.0` 和 `PORT`).

### 平台

- **Fly.io**: 支持 Docker 部署，适合 Rust 应用。
- **Shuttle/AWS/GCP**: 其他云服务提供商。

## 14. 下一步 (Next Steps)

- **深入学习**: 阅读 [Essentials](https://dioxuslabs.com/learn/0.7/essentials/) 章节，了解更深层的响应式原理。
- **生态系统**: 探索 Dioxus 生态中的其他库。
- **实践**: 尝试为 HotDog 添加用户登录、动画或更多功能。

---
*本文档基于 Dioxus 0.7 官方教程编译。*
