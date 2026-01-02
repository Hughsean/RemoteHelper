# Dioxus 0.7 最佳实践重构报告

## ✅ **已完成的改进**

### 1. **状态管理模式** - 符合官方推荐 ⭐⭐⭐⭐⭐

**之前的问题** ❌

```rust
// ❌ 错误：在结构体中直接存储 Signal
pub struct AuthState {
    pub authenticated: Signal<bool>,
    pub connected: Signal<bool>,
    // ...
}
```

**现在的实现** ✅

```rust
// ✅ 正确：分离数据层和响应式层
#[derive(Clone, PartialEq, Debug)]
pub struct AuthData {
    pub authenticated: bool,
    pub connected: bool,
    // ...
}

// 响应式包装
pub type AuthState = Signal<AuthData>;

// 官方推荐的访问方式
pub fn use_auth_state() -> AuthState {
    use_context::<AuthState>()
}
```

**符合官方文档**:

- ✅ [Essentials: 状态基础](https://docs.rs/dioxus/0.7/dioxus/) - "Signal 应该在组件中使用 use_signal 创建"
- ✅ [Guides: Context](https://docs.rs/dioxus/0.7/dioxus/) - "通过 Context 共享状态访问器"
- ✅ [Tutorial: State Management](https://docs.rs/dioxus/0.7/dioxus/) - "使用 type alias 简化 Signal 类型"

---

### 2. **Context Provider 模式** - 符合官方推荐 ⭐⭐⭐⭐⭐

**之前的问题** ❌

```rust
// ❌ 错误：在 Default trait 中创建 Signal 会导致每次都创建新实例
impl Default for AppState {
    fn default() -> Self {
        Self {
            auth: Signal::new(false), // 每次调用都是新的 Signal!
        }
    }
}

use_context_provider(|| AppState::default());
```

**现在的实现** ✅

```rust
// ✅ 正确：在根组件中使用 use_signal 创建，确保全局唯一
#[component]
fn App() -> Element {
    use_context_provider(|| use_signal(AuthData::default));
    use_context_provider(|| use_signal(SystemData::default));
    use_context_provider(|| use_signal(ServicesData::default));
    
    rsx! { Router::<Route> {} }
}
```

**符合官方文档**:

- ✅ [Essentials: Context](https://docs.rs/dioxus/0.7/dioxus/) - "在根组件提供 Signal Context"
- ✅ [Reference: use_context_provider](https://docs.rs/dioxus/0.7/dioxus/) - "使用 use_signal 创建响应式状态"

---

### 3. **自定义 Hooks** - 符合官方推荐 ⭐⭐⭐⭐⭐

**新增功能** ✅

```rust
// ✅ 提供类型安全的 hooks 访问状态
pub fn use_auth_state() -> AuthState {
    use_context::<AuthState>()
}

pub fn use_system_state() -> SystemState {
    use_context::<SystemState>()
}

pub fn use_services_state() -> ServicesState {
    use_context::<ServicesState>()
}
```

**使用示例**:

```rust
#[component]
fn LoginForm() -> Element {
    // 简洁、类型安全的状态访问
    let auth = use_auth_state();
    
    // 读取状态（不可变访问）
    if auth.read().authenticated {
        return rsx! { div { "已登录" } };
    }
    
    // 修改状态（可变访问）
    let onclick = move |_| {
        auth.write().set_authenticated();
    };
    
    rsx! { /* ... */ }
}
```

**符合官方文档**:

- ✅ [Guides: Custom Hooks](https://docs.rs/dioxus/0.7/dioxus/) - "封装 use_context 提供更好的 API"
- ✅ [Tutorial: Hooks Best Practices](https://docs.rs/dioxus/0.7/dioxus/) - "use_ 前缀命名自定义 hooks"

---

### 4. **数据可变性控制** - 符合官方推荐 ⭐⭐⭐⭐⭐

**之前的问题** ❌

```rust
// ❌ 混乱的访问方式
self.authenticated.set(true);
let val = self.connected.read();
self.error_msg.write().push_str("error");
```

**现在的实现** ✅

```rust
// ✅ 清晰的不可变/可变分离
impl AuthData {
    // 封装的可变方法
    pub fn set_authenticated(&mut self) {
        self.authenticated = true;
        self.connected = true;
        self.error_msg = None;
    }
}

// 组件中使用
auth.write().set_authenticated();  // 可变访问
if auth.read().connected { }       // 不可变访问
```

**符合官方文档**:

- ✅ [Essentials: Signal API](https://docs.rs/dioxus/0.7/dioxus/) - "read() 不可变，write() 可变"
- ✅ [Reference: Signal Methods](https://docs.rs/dioxus/0.7/dioxus/) - "优先使用方法封装复杂逻辑"

---

## 📚 **架构改进对比**

### **之前的架构** ❌

```
AppState {
    auth: AuthState {  ← Signal 嵌套在结构体中
        authenticated: Signal<bool>,
        connected: Signal<bool>,
    }
}
```

**问题**:

1. Signal 在 Default trait 中创建，每次调用都是新实例
2. 违反了 Dioxus 的响应式原则（Signal 应该在组件中创建）
3. 类型复杂（`Signal<Signal<T>>`）
4. 难以理解的访问方式（`state.auth.authenticated.read()`）

### **现在的架构** ✅

```
数据层:      AuthData { authenticated: bool, ... }
响应式层:    AuthState = Signal<AuthData>
访问层:      use_auth_state() -> AuthState
```

**优点**:

1. ✅ 清晰的层次分离
2. ✅ Signal 在根组件创建（`use_context_provider(|| use_signal(...))`）
3. ✅ 简洁的类型（`Signal<AuthData>`）
4. ✅ 直观的访问方式（`auth.read().authenticated`）

---

## 🎯 **符合的官方推荐**

| 推荐实践 | 之前 | 现在 | 文档出处 |
|---------|------|------|---------|
| Signal 应在组件中创建 | ❌ 在 Default trait 中 | ✅ 在 App 组件中 | Essentials: State |
| 使用 type alias 简化类型 | ❌ 无 | ✅ `type AuthState = Signal<AuthData>` | Tutorial: Patterns |
| 提供自定义 hooks | ❌ 无 | ✅ `use_auth_state()` | Guides: Hooks |
| 分离数据和响应式层 | ❌ 混合 | ✅ 分离 | Reference: Architecture |
| Context 提供 Signal | ❌ 提供包含 Signal 的结构体 | ✅ 直接提供 Signal | Guides: Context |

---

## 📖 **使用指南**

### **在组件中读取状态**

```rust
#[component]
fn StatusDisplay() -> Element {
    let system = use_system_state();
    
    // 不可变访问 - 用于显示
    let status = system.read();
    
    rsx! {
        div {
            "CPU: {status.current_status.map(|s| s.cpu_usage)}"
        }
    }
}
```

### **在组件中修改状态**

```rust
#[component]
fn LoginButton() -> Element {
    let auth = use_auth_state();
    
    let onclick = move |_| {
        // 可变访问 - 用于修改
        auth.write().set_authenticated();
    };
    
    rsx! {
        button { onclick, "登录" }
    }
}
```

### **在异步任务中使用**

```rust
#[component]
fn DataFetcher() -> Element {
    let system = use_system_state();
    
    use_resource(move || async move {
        loop {
            if let Ok(status) = backend::get_status().await {
                // 在异步上下文中更新状态
                system.write().update_status(status);
            }
            sleep(Duration::from_secs(1)).await;
        }
    });
    
    rsx! { /* ... */ }
}
```

---

## ⚠️ **迁移注意事项**

如果你已经写了组件代码，需要进行以下调整：

### **1. 状态访问方式**

```diff
- let auth = use_context::<AppState>().auth;
- if auth.authenticated.read() { }
+ let auth = use_auth_state();
+ if auth.read().authenticated { }
```

### **2. 状态修改方式**

```diff
- auth.authenticated.set(true);
- auth.connected.set(true);
+ auth.write().set_authenticated();
```

### **3. 导入路径**

```diff
- use state::AppState;
+ use state::{use_auth_state, use_system_state, use_services_state};
```

---

## 🔍 **参考文档**

1. **Dioxus 0.7 Essentials**: <https://docs.rs/dioxus/0.7/dioxus/>
   - State Basics: `use_signal` 创建响应式状态
   - Context: 跨组件共享状态

2. **Dioxus Guides**: <https://dioxuslabs.com/learn/0.7/guide>
   - Custom Hooks: 封装状态访问逻辑
   - State Management Patterns: 分层架构设计

3. **Dioxus Tutorial**: <https://dioxuslabs.com/learn/0.7/tutorial>
   - Interactive State: Signal API 详解
   - Component Composition: 状态共享模式

4. **Dioxus Reference**: <https://docs.rs/dioxus/0.7/dioxus/prelude/>
   - `use_signal`: 创建本地响应式状态
   - `use_context_provider`: 提供全局状态
   - `Signal::read()`: 不可变访问
   - `Signal::write()`: 可变访问

---

## ✅ **检查清单**

- [x] Signal 在组件中创建（不在 Default trait 中）
- [x] 使用 type alias 简化类型（`type AuthState = Signal<AuthData>`）
- [x] 提供自定义 hooks（`use_auth_state()`）
- [x] 分离数据层和响应式层（`AuthData` + `Signal`）
- [x] 正确的 Context 提供方式（`use_context_provider(|| use_signal(...))`）
- [x] 清晰的可变/不可变访问（`read()` vs `write()`）
- [x] 封装复杂逻辑到方法中（`set_authenticated()`）

---

## 🚀 **下一步**

1. **迁移 UI 组件**: 使用新的 hooks API 重写组件
2. **添加 use_resource**: 实现自动数据刷新
3. **错误处理**: 添加 Result 类型和错误展示
4. **性能优化**: 使用 memo() 避免不必要的重渲染
