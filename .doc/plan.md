任务描述：
将现有的 app 项目迁移到 app-rust 项目，采用最新的 Dioxus 技术栈和模块化设计。确保代码结构清晰，功能模块化，遵循 Dioxus 官方最佳实践。

迁移要求：

模块化设计：

将功能拆分为多个模块（如组件、状态管理、服务等）。
每个模块单独维护，避免所有代码集中在一个文件中。
技术栈对比：

app 项目使用 Vite + TypeScript + TailwindCSS。
app-rust 项目使用 Dioxus + Rust，需替换前端框架相关代码。
Dioxus 组件化：

使用最新的 Dioxus 组件库和文档（参考：https://dioxuslabs.com/components）。
将页面拆分为多个可复用的组件（如导航栏、表单、数据展示等）。
迁移：
替换 @tauri-apps/api 的调用逻辑，使用 Dioxus 和 Tauri 的 Rust API。

日志和调试：

集成 dioxus_logger，设置日志级别为 INFO。
确保迁移后的项目支持调试和错误追踪。

测试和验证：

确保迁移后的项目功能完整，样式一致。
使用 Dioxus 提供的工具进行调试和优化。
