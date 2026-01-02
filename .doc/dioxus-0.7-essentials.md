# Dioxus 0.7 核心概念 (Essentials)

Dioxus 是一个用于构建跨平台应用程序的 Rust 框架，只需一套代码库。每个应用程序都利用用户界面 (UI) 来显示内容并允许用户采取行动。UI 由按钮、文本和图像等小单元构建而成，然后组织成封装功能的组件。Dioxus 应用程序是通过将组件组合成更大的交互树来构建的。

![Widget Tree](https://dioxuslabs.com/assets/widget-tree-dxhfde2b285eb568ccb.webp)

使用 Dioxus，你只需声明你希望应用程序的外观，轻量级的 Dioxus 运行时即可确保你的小部件在 Web、桌面和移动设备上正确渲染。

## 高级概述

Dioxus 建立在数十年的 UI 研究基础之上。构建交互式、美观且高效的用户界面可能具有挑战性。我们设计 Dioxus 是为了通过一个简单的思维模型来解决前端开发的复杂性：

1. **声明式构建 UI**：描述小部件的布局和样式。
2. **状态存储**：应用程序的状态存储在称为 "hooks" 的小型可组合函数中。
3. **响应式更新**：状态的修改是对用户交互的响应，这会导致 "重新渲染"。

手动创建和修改 UI 元素可能既乏味又容易出错。Dioxus 使用的声明式 UI 方法需要一些训练才能掌握，但最终会产生高效且易于维护的前端代码。我们建议彻底通读核心概念，以了解 Dioxus UI 和响应式模型的工作原理。

---

# 构建用户界面 (Building User Interfaces)

欢迎来到 Dioxus！本章将教你如何使用 Rust 和 Dioxus 构建漂亮的用户界面。

Dioxus 允许你使用 HTML 和 CSS 编写用户界面。在 Web 上，你的 UI 是原生的且可访问的；在桌面和移动设备上，你的小部件要么在 webview 中渲染，要么使用混合原生组件进行原生渲染。

HTML 和 CSS 允许你构建非常漂亮、丰富、交互式的体验。世界上顶尖的公司都在其网站和应用程序中使用 HTML 和 CSS。

## Dioxus 提供了什么

Dioxus 为你提供了利用 HTML、CSS 和 Rust 全部光彩所需的所有工具。

这包括：

* Rust 代码、UI、样式和资产的**热重载 (Hot-reloading)**
* Web、桌面和移动设备的**跨平台渲染器**
* 用于管理和更新状态的**响应式系统**
* 不断增长的**跨平台 API SDK**
* 用于构建全栈 Web 应用程序的**后端集成**
* 用于打包和部署到生产环境的**工具**

你可以在 Dioxus 中做任何你可以用 HTML 和 CSS 做的事情。

## Rust 提供了什么

为什么要用 Rust 编写 UI 框架？为什么要用 Rust 构建应用程序？Rust 最初是为系统编程而设计的，尽管我们在 Dioxus 致力于使其适合像应用程序开发这样的高级编程。

Rust 的学习曲线相当陡峭，但好处是巨大的：

* 出色的标准化工具 (`cargo`, `rustc`)
* 庞大的生态系统和出色的包管理器 (`crates.io`, `Cargo.toml`)
* 随处运行 - Mac, Windows, Linux, iOS, Android, Web 等
* 强大的类型系统，可在编译时防止许多逻辑错误
* 出色的性能，可与 C / C++ 等语言相媲美
* 即使在大规模下也可靠

---

# RSX 简介 (Introducing RSX)

Dioxus 应用程序由组成组件的小元素组成，然后组装成树。为了构建我们的组件，我们结合使用 Rust 代码和一种我们称之为 "RSX" 的简化 Rust 方言。

RSX 旨在看起来和感觉像 HTML 和 SwiftUI 的混合体：

```rust
rsx! {
    h1 { "Welcome to Dioxus!" }
    h3 { "Brought to you by {author}" }
    p { class: "main-content", {article.content} }
}
```

目前，RSX 是开发人员用来构建 Dioxus 应用程序的主要语法。还有其他选择，但 RSX 拥有最好的开发人员工具，如即时热重载、代码折叠和语法高亮。

## `rsx!` 宏

如果你熟悉 React 或 Vue 等库，你可能会熟悉 JSX 标记语言。JSX 是 JavaScript 的一种替代形式，允许开发人员在一个文件中混合 JavaScript 代码和 XML。这些库依赖编译器插件将语法转换为纯 JavaScript 代码，然后渲染 HTML。

在 Rust 中，我们通过使用[过程宏 (procedural macros)](https://doc.rust-lang.org/reference/procedural-macros.html) (proc macros) 来实现类似的体验。过程宏是微小的编译器插件，可将 Rust token 转换为 Rust 代码。你可以通过 `!` 修饰符识别出一个函数是过程宏。例如，`rsx!` 宏将 RSX 语法转换为 Rust 代码：

```rust
// 这个宏...
rsx! {
    div { "hello {world}!" }
}

// 扩展为这个模板：
static TEMPLATE: Template = Template {
    nodes: [
        ElementNode {
            tag: div,
            children: [
                TextNode {
                    contents: DynamicText(0)
                }
            ]
        }
    ]
}
TEMPLATE.render([
    format!("hello {world}")
])
```

RSX 为我们做了很多繁重的工作，大大减少了声明 UI 的冗长。它还以最有效的表示形式构建我们的 UI，使渲染极其快速。

## Dioxus 渲染 HTML 和 CSS

我们希望确保 Dioxus 易于学习且极其便携。我们没有发明新的样式、布局和标记系统，而是选择简单地在任何地方都依赖 HTML 和 CSS。对于 Web，这很方便 - 网站已经是用 HTML 和 CSS 构建的。在桌面和移动设备上，我们提供了一个渲染器，可以自动将你的 HTML 和 CSS 转换为原生小部件。

我们的混合 HTML 方法结合了 Flutter 和 React Native 等框架的最佳部分。团队可以获得最大的代码重用，使用熟悉的标记语言，并且 AI 工具可以立即提供帮助。团队无需为每个平台构建定制的原生应用程序，只需编写一次组件即可在任何地方渲染。

我们的渲染引擎 [Blitz](https://github.com/dioxuslabs/blitz) 是开源的，通常与浏览器级引擎无法区分。

---

# 组件 (Components)

在 Dioxus 中，组件是简单的函数，封装了 UI 的表示、状态和交互。随着应用程序规模的增长，请考虑将共享功能组合成更小的可重用组件。模块化组件使大型团队的工作更轻松，防止恼人的错误，并实现跨所有平台的更好的代码重用。

## 定义组件

以最简单的形式，组件是一个返回 `Element` 的函数。例如，基本的 `app` 组件：

```rust
fn app() -> Element {
    rsx! { "hello world!" }
}
```

如果你想从另一个组件使用一个组件，那么你必须用 `#[component]` 宏注释该函数。此宏为 RSX 宏提供必要的元数据，允许函数参数成为组件属性中的字段。你的函数必须满足以下要求：

* 以大写字母开头 (`MyComponent`) 或包含下划线 (`my_component`)
* 接受实现 [PartialEq](https://doc.rust-lang.org/std/cmp/trait.PartialEq.html) 和 [Clone](https://doc.rust-lang.org/std/clone/trait.Clone.html) 的参数
* 返回一个 [Element](https://docs.rs/dioxus/latest/dioxus/prelude/type.Element.html)

我们将组件接受的参数称为其**属性 (properties)**。属性用于将数据传递到组件中，类似于将参数传递给函数的方式。

例如，我们可以定义一个简单的组件，它接受一个 `name` 属性并渲染问候语：

```rust
#[component]
pub fn MyComponent(name: String) -> Element {
    rsx! {
        div {
            h3 { "Hello, {name}!" }
        }
    }
}
```

## 在 RSX 中使用组件

定义组件后，你可以像使用任何其他元素一样在 RSX 标记中使用它：

```rust
rsx! {
    MyComponent { name: "World" }
}
```

虽然组件被定义为函数，但不应像常规函数一样调用它们。当你在 RSX 中使用组件时，Dioxus 将创建一个控制其自身状态和生命周期的组件新实例。

## 组件属性 (Component Properties)

组件的属性是在渲染时传递给组件的对象。这些属性类似于函数的参数 - 这就是为什么属性字段在组件中被定义为函数参数。在某些情况下，你可能会发现将组件属性提取到单独的结构体中很有用。我们可以简单地使用名为 "props" 的参数，然后在随附的结构体中定义 props，而不是内联定义属性。

```rust
#[derive(PartialEq, Clone, Props)]
struct CardProps {
    title: String,
    content: String
}

#[component]
fn Card(props: CardProps) -> Element {
    rsx! {
        h1 { "{props.title}" }
        span { "{props.content}" }
    }
}
```

当我们提取属性到结构体时，我们需要确保结构体实现了三个必需的 trait：

* `PartialEq`：用于确定组件是否需要重新渲染
* `Clone`：在每次渲染时，属性对象都会被克隆
* `Props`：派生 `Properties` trait，Dioxus 使用它进行渲染和构建

### PartialEq

`PartialEq` 被 Dioxus 运行时用来确定组件的属性是否因某些用户操作而发生了变化。因为组件被其他组件调用，组件树上游的微小变化可能会导致 Dioxus 运行时进行级联工作。`PartialEq` 实现用于在响应这些变化时最小化 Dioxus 运行时需要做的工作。

### Clone

`Clone` 约束是组件属性的一个特别有趣的要求。Dioxus 组件树架构是围绕一种称为 "单向数据流" 的思想设计的。这是一种设计模式，可防止组件树下游的组件修改其输入属性。通过防止这些意外突变，用户界面通常会有更少的错误。

通常，`.clone()` 大多数组件属性是可以的，但某些对象可能克隆起来太昂贵并会对性能产生负面影响。在大多数情况下，你可以简单地将值包装在 [`ReadSignal`] 包装器类型中，对象将自动包装在智能指针中：

```rust
// 之前...
struct CardProps {
    content: String
}

// 使用 ReadSignal:
struct CardProps {
    content: ReadSignal<String>
}
```

### Props

`Props` 派生宏为属性对象实现 `Properties` trait。此 trait 及其实现是 Dioxus 中的实现细节，通常不意味着手动实现。`Props` 派生很有用，因为它为 Properties 派生了一个强类型的构建器。

## `#[component]` 宏

可以修改属性以接受比普通函数参数更广泛的输入。

例如，Dioxus Router `Link` 组件广泛使用了修饰符：

```rust
/// Link 的属性
#[derive(Props, Clone, PartialEq)]
pub struct LinkProps {
    // ...
    /// 当为 true 时，目标路由将在新标签页中打开。
    #[props(default)]
    pub new_tab: bool,

    /// 导航目标。
    #[props(into)]
    pub to: NavigationTarget,
    // ...
}
```

你可以在每个字段上使用 `#[props()]` 属性来修改组件接受的属性：

* `#[props(default)]` - 使字段在组件中可选，如果未设置则使用默认值。
* `#[props(!optional)]` - 使类型为 `Option<T>` 的字段成为必填项。
* `#[props(into)]` - 使用 `Into` trait 将字段转换为正确的类型。
* `#[props(extends = GlobalAttributes)]` - 使用元素或全局元素属性的所有属性扩展 props。

## 展开 Props (Spreading Props)

为了获得更好的可组合性，我们可以手动创建组件的属性，然后使用 Rust 的展开语法 `..some_props` 直接传递它们。

```rust
let props = CardProps::lorem_ipsum();
rsx! {
    Card { ..props }
}
```

## 子元素 (Children)

属性有一个名为 "children" 的特殊字段，其中包含组件的子元素。例如，我们可以构建一个包装器组件，将其子元素包装在红色 div 中。此组件接受一个名为 "children" 的特殊参数，它是一个在 RSX 表达式中使用的 `Element`。

```rust
#[component]
fn RedDiv(children: Element) {
    rsx! {
        div {
            background_color: "red",
            {children}
        }
    }
}
```

调用组件时，我们可以像元素一样简单地添加嵌套子元素：

```rust
rsx! {
    RedDiv {
        h1 { "Lorem Ipsum Dolor" }
        p { "..." }
    }
}
```

---

# 条件渲染 (Conditional Rendering)

我们的用户界面到目前为止一直非常静态。然而，我们使用 Dioxus 构建的大多数应用程序通常包含大量动态内容。我们的 UI 将对按钮、表单输入、滑块或外部数据源（如网络）的变化做出反应。

## 表达式

就像 JSX 一样，RSX 允许你使用纯 Rust 代码轻松地将 `Element` 对象组合在一起。你可以直接在 RSX 中编写 Rust 表达式。只要表达式计算结果为 `Element` 或任何实现 `IntoDynNode` 的东西，你就可以简单地将其包装在大括号 (`{}`) 中：

```rust
let content = "world!";
rsx! {
    h1 {
        "Hello"
        {content}
    }
}
```

或者，我们可能希望动态渲染一些 RSX 并将其分配给变量：

```rust
let header = match current_timezone() {
    TimeZone::PST => rsx! {
        h1 { "Welcome home" }
    },
    _ => rsx! {
        h1 { "Bon voyage!" }
    },
}

rsx! {
    div {
        {header}
    }
}
```

在 Dioxus 中，你可以简单地使用 if/else 语句：

```rust
let screen = if authenticated { render_app() } else { render_login() };
rsx! {
    div {
        {screen}
    }
}
```

## IntoDynNode Trait

Dioxus 使用 `IntoDynNode` trait 来确定表达式是否可以在 RSX 中使用。转换将采用 Rust 表达式并将其转换为四个 `DynamicNode` 变体之一：

* **Component**: 接受 Properties 并渲染 Element 的函数
* **Text**: Rust `String` 类型
* **Placeholder**: 优化的 `None` 值
* **List**: Elements 的 Vec

## 内联 If 语句

当渲染源自布尔条件（例如，"active" 或 "inactive"）的内容时，RSX 提供了一些小的 "语法糖"，可在 RSX 中启用内联 `if` 语句。

```rust
let logged_in = use_signal(|| false);

rsx! {
    div {
        if logged_in() {
            "You are logged in"
        } else {
            "You are not logged in"
        }
    }
}
```

注意，内联 `if` 语句的主体是 RSX，而不是 Rust 表达式。

内联 `if` 语句在一个方面偏离了 Rust：即使没有 `else` 分支，它们仍然计算为 `Element`。如果 RSX 在你的 `if` 语句中找不到 `else` 分支，它会自动返回一个占位符元素。

---

# 列表渲染 (Rendering Lists)

## 迭代器和内联 for

任何返回 `Element` 或实现 `IntoVnode` 的对象的迭代器都可以在 RSX 块内用于渲染列表：

```rust
rsx! {
    // 映射现有迭代器
    {(0..10).map(|idx| rsx! { "item {idx}" })}

    // 或者调用 .iter()
    {users.iter().map(|user| rsx!{ User { id: user.id } })}
}
```

Dioxus 提供了一些语法糖，使使用迭代器更好一些。你可以将其移动到内联 `for` 块中，而不是将迭代器包装在表达式中：

```rust
rsx! {
    for idx in 0..10 {
        "Item {idx}"
    }

    for user in users.iter() {
        User { id: user.id }
    }
}
```

## 必须使用 Key

列表中的每个项目都应该有一个在重新渲染之间稳定的唯一值，称为 key。Key 用于识别项目在渲染之间如何移动。如果没有 key，当你重新排序列表中的项目时，很容易意外丢失或移动状态。我们可以通过使用 `key` 属性向列表项添加 key：

```rust
rsx! {
    ul {
        for item in items.iter() {
            li { key: "{item}", "{item}" }
        }
    }
}
```

通常可以从状态本身找到合适的 key（例如唯一的 ID）。最糟糕的 key 是项目在集合中的索引。

## Fragment 组件

在某些情况下，你的迭代器可能会返回多个根元素。这并没有给我们一个放置 key 的好位置。RSX 自动使用第一个元素的 key 作为迭代器 key。如果没有简单的方法将 key 附加到第一个元素，你可以使用 `Fragment` 组件来包装元素并赋予其 key：

```rust
rsx! {
    for item in items.iter() {
        Fragment {
            key: "{item.id}",
            for child in item.children.iter() {
                div { "{child}" }
            }
        }
    }
}
```

---

# 状态基础 (The Basics of State)

现在你知道了如何在 Dioxus 中创建用户界面，是时候学习如何创建和更新应用程序的状态了。

## 状态管理理论

最终，"状态管理" 指的是：

1. 为 UI 初始化数据
2. 处理来自用户的事件
3. 更新数据并重新渲染 UI

## 对于经验丰富的 Web 开发人员

Dioxus 使用基于 Signal 的响应式系统。与 SolidJS 不同，值的读取和写入是显式的。Rust 没有相当于 JavaScript Proxy 的东西，所以响应性是通过调用 `.read()` 和 `.write()` 来追踪的。

```rust
let mut count = use_signal(|| 0);

rsx! {
    button {
        onclick: move |_| *count.write() += 1,
        "Increment"
    }
    {count.read().to_string()}
}
```

## 对于经验丰富的 Rust 开发人员

如果你是作为经验丰富的 Rust 开发人员来到 Dioxus，你可能会被我们使用 "不寻常" 的原语（如 `use_signal`, `use_memo`, `use_resource` 等）所吓倒。Dioxus 使用有状态的 "hooks" - 一种起源于 Web 开发（特别是 React）的范式。

如果你不想在 Dioxus 中编写类似 React 的代码，你可以选择使用结构体来存储状态并命令式地处理 UI 更新。

```rust
struct EditorState {
    text: String
}

impl EditorState {
    fn handle_input(&mut self, event: FormEvent) {
        self.text = event.value();
    }
}

#[component]
fn TextEditor() -> Element {
    // 使用单个 "state" 对象，包装在 `Signal` 中
    let mut state = use_signal(EditorState::new);
    
    rsx! {
        input {
            // 在事件处理程序中，调用 `.write()` 以获取 "state" 对象上的 `&mut self`
            oninput: move |event| state.write().handle_input(event)
        }
    }
}
```

---

# 全栈开发 (Fullstack)

几乎所有应用程序都需要远程服务器来存储和更新用户数据。Dioxus 提供了许多全栈实用程序，用于在客户端旁边构建应用程序的服务器。使用 Dioxus Fullstack，你可以完全用 Rust 构建应用程序的前端和后端！

Dioxus Fullstack 与流行的 [Axum](https://docs.rs/axum/latest/axum/) 框架深度集成。

## 热重载 (Hot-Reload)

Dioxus Fullstack 内置了完整的 Rust 热重载支持，这要归功于我们的热补丁引擎 [subsecond](https://crates.io/crates/subsecond)。Subsecond 使用先进的汇编和链接器技术允许在运行时修改 Rust 函数。

## 服务器函数 (Server functions)

服务器函数允许你定义一个始终在服务器上运行的函数。当你从客户端调用该函数时，Dioxus 将自动序列化参数，将其发送到服务器，在服务器上运行该函数，序列化返回值，并将其发送回客户端。

```rust
// 函数体将始终在服务器上运行，因此我们可以执行数据库查询等服务器端操作
#[get("/api/dog/{breed}")]
async fn fetch_dog(breed: String) -> Result<String> {
    DB.execute("SELECT url FROM dogs WHERE id = ?1", &breed)
}
```

## 服务端渲染 (Server Side Rendering)

Dioxus Fullstack 允许你在服务器上渲染应用程序，加快用户的加载时间并提高网站在 Google 等搜索引擎上的可发现性。服务端渲染 (SSR) 允许你在服务器上渲染应用程序的初始 HTML，将格式完整的 HTML 文档发送到客户端。然后客户端可以将 HTML "水合 (hydrate)" 为完全交互式的应用程序。

---

# 路由 (Routing)

随着应用程序的增长，将应用程序组织成多个页面或视图并在它们之间切换会很有帮助。Dioxus 路由器帮助你管理应用程序的 URL 状态。

## 安装路由器

```toml
[dependencies]
dioxus = { version = "0.7", features = ["router"] }
```

## 创建 Routable 枚举

路由器的核心是你的 `Routable` 枚举。你将在整个应用程序中使用此枚举来导航到不同的页面。

```rust
use dioxus::prelude::*;

#[derive(Clone, Debug, PartialEq, Routable)]
enum Route {
    #[route("/")]
    Home,

    #[route("/about")]
    About,

    #[route("/user/:id")]
    User { id: u32 },
}
```

## 渲染路由器

```rust
fn main() {
    dioxus::launch(|| rsx! { Router::<Route> {} });
}
```

## 链接到你的第一个路由

要导航到不同的路由，你可以使用路由器提供的 `Link` 组件。

```rust
#[component]
fn Home() -> Element {
    rsx! {
        div {
            "Welcome to the home page!"
            Link { to: Route::About, "Go to About Page" }
        }
    }
}
```

---

# 高级主题 (Advanced Topics)

本节涵盖了随着 Dioxus 应用程序增长你可能会发现有用的各种高级主题。

* **自定义 Hooks (Custom hooks)**：除了内置 hooks 之外，Dioxus 还允许你创建自己的自定义 hooks 来封装可在整个组件中使用的逻辑。
* **组件生命周期 (Component Lifecycle)**：Dioxus 中的每个组件都遵循相同的挂载、diff 和 drop 生命周期。
* **突破限制 (Breaking Out)**：有时 Dioxus 提供的 API 不够用。本节介绍如何突破 Dioxus 并使用你自己的 API 来操作 DOM 或调用 JS 函数。
