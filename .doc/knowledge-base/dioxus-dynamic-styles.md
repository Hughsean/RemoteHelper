# Dioxus 动态样式实现指南

## 概述

在 Dioxus 框架中，实现动态样式有多种方式。本文档总结了在开发过程中遇到的问题和最佳实践。

## 问题背景

在实现路径自动补全功能时，需要为选中的建议项添加视觉反馈（高亮显示）。最初尝试使用外部 CSS 类和 `style` 字符串属性，但遇到了样式不生效的问题。

## 解决方案

### 方法一：使用 Dioxus 原生样式属性（推荐）

Dioxus 提供了直接的样式属性，可以在 RSX 中使用条件表达式：

```rust
rsx! {
    div {
        class: "base-class",
        // 使用条件表达式直接设置样式属性
        background_color: if is_selected { "#4f46e5" },
        border_left: if is_selected { "3px solid #818cf8" },
        "Content"
    }
}
```

**优点：**

- 样式直接应用，无需担心 CSS 加载顺序
- 类型安全，编译时检查
- 响应式更新，与 signal 配合良好
- 性能更好，直接操作 DOM

**Dioxus 支持的样式属性包括：**

- `background_color` → `background-color`
- `border_left` → `border-left`
- `border_right` → `border-right`
- `border_top` → `border-top`
- `border_bottom` → `border-bottom`
- `font_size` → `font-size`
- `padding`, `margin`, `width`, `height` 等
- 完整列表见：`packages/html/src/attribute_groups.rs`

### 方法二：使用 style 字符串属性

如果需要应用多个样式属性，可以使用字符串形式：

```rust
let style_str = if is_selected {
    "background-color: #4f46e5; border-left: 3px solid #818cf8;"
} else {
    ""
};

rsx! {
    div {
        class: "base-class",
        style: "{style_str}",
        "Content"
    }
}
```

**注意事项：**

- 字符串必须是合法的 CSS 语法
- 多个属性用分号分隔
- 属性名使用 kebab-case（如 `background-color`）

### 方法三：动态 class（外部 CSS）

使用条件表达式动态应用 CSS 类：

```rust
rsx! {
    div {
        class: "base-class",
        class: if is_selected { "selected" },
        "Content"
    }
}
```

**外部 CSS：**

```css
.suggestion-item {
    padding: 0.5rem;
    cursor: pointer;
}

.suggestion-item.selected {
    background-color: #4f46e5;
    border-left: 3px solid #818cf8;
}
```

**问题：**
在某些情况下，外部 CSS 类可能不会立即生效，特别是在：

- 列表动态渲染时
- Signal 快速更新时
- CSS 文件加载时序问题

## 最佳实践

### 1. 选择合适的方法

- **简单样式变化**：使用原生样式属性（方法一）
- **复杂样式组合**：使用外部 CSS + 动态 class（方法三）
- **需要字符串插值**：使用 style 属性（方法二）

### 2. 避免在循环中读取 Signal

❌ **错误示例：**

```rust
for (idx, item) in items().iter().enumerate() {
    let is_selected = idx == selected_index(); // 每次迭代都读取 signal
    // ...
}
```

✅ **正确示例：**

```rust
let current_selected = selected_index(); // 循环外读取一次
for (idx, item) in items().iter().enumerate() {
    let is_selected = idx == current_selected;
    // ...
}
```

### 3. 使用 key 属性

在列表渲染时必须使用 `key` 属性，帮助 Dioxus 跟踪元素：

```rust
for (idx, item) in items().iter().enumerate() {
    rsx! {
        div {
            key: "{item.id}", // 使用唯一标识
            // 不要使用索引作为 key（除非列表不会重排）
        }
    }
}
```

### 4. 条件属性语法

Dioxus 支持未终止的 if 语句作为属性值：

```rust
rsx! {
    div {
        // 只有条件为真时才应用该属性
        hidden: if !visible,
        disabled: if is_loading,
        background_color: if is_active { "blue" } else { "gray" },
    }
}
```

## 调试技巧

### 1. 使用浏览器控制台日志

```rust
use gloo_console::log;

log!("Rendering item", idx, "selected:", is_selected);
```

### 2. 检查生成的 HTML

在浏览器开发者工具中检查：

- 元素的 class 属性是否正确
- style 属性是否存在
- 计算后的样式（Computed styles）

### 3. 验证 CSS 加载

确保 CSS 文件已正确加载：

```rust
rsx! {
    Stylesheet { href: asset!("/assets/styles.css") }
}
```

## 性能考虑

1. **避免不必要的重新渲染**
   - 使用 `use_memo` 缓存计算结果
   - 合理拆分组件

2. **批量更新样式**
   - 多个样式变化时，考虑使用 CSS 类切换而不是多个独立属性

3. **Signal 使用**
   - 避免在渲染循环中多次读取同一个 signal
   - 使用 `read()` 或直接调用获取值

## 常见陷阱

### 陷阱 1：CSS 特异性问题

如果外部 CSS 不生效，可能是特异性不够：

```css
/* 不够特异 */
.selected {
    background-color: blue;
}

/* 更特异 */
.suggestion-item.selected {
    background-color: blue !important;
}
```

### 陷阱 2：use_memo 不自动跟踪依赖

Dioxus 的 `use_memo` 需要手动指定依赖：

```rust
// 错误：use_memo 不会自动跟踪 selected_index
let is_selected = use_memo(move || idx == selected_index());

// 正确：直接在循环中比较
let is_selected = idx == selected_index();
```

### 陷阱 3：字符串插值语法

在 style 属性中使用变量：

```rust
let color = "#4f46e5";

// ❌ 错误
style: "background-color: {color}",

// ✅ 正确
style: "background-color: {color}",  // 实际上两者一样，确保在格式化字符串中
```

## 参考资源

- [Dioxus 官方文档 - RSX](https://dioxuslabs.com/learn/0.6/essentials/rsx/)
- [Dioxus GitHub - HTML 属性组](https://github.com/DioxusLabs/dioxus/tree/main/packages/html/src/attribute_groups.rs)
- [MDN - CSS 参考](https://developer.mozilla.org/en-US/docs/Web/CSS)

## 总结

在 Dioxus 中实现动态样式时：

1. **优先使用原生样式属性**（如 `background_color`）获得最佳性能和可靠性
2. **避免过度依赖外部 CSS 类**的动态切换，特别是在快速更新的场景
3. **在循环外读取 Signal 值**避免不必要的性能开销
4. **使用合适的 key 属性**确保列表正确更新
5. **结合浏览器开发者工具**进行调试

这些实践经验来自实际开发中遇到的问题和解决方案，可以有效避免常见的样式问题。
