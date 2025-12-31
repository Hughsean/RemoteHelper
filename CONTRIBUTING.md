# 贡献指南

感谢您对 RemoteHelper 项目的关注！我们欢迎各种形式的贡献。

## 🤝 如何贡献

### 报告 Bug

如果您发现了 bug，请创建一个 Issue，并提供：

1. **问题描述**：清晰描述遇到的问题
2. **复现步骤**：如何重现该问题
3. **预期行为**：应该发生什么
4. **实际行为**：实际发生了什么
5. **环境信息**：
   - 操作系统版本
   - Rust 版本 (`rustc --version`)
   - 项目版本
   - 相关日志输出

### 提出功能建议

我们欢迎新功能建议！请创建 Issue 并说明：

1. **功能描述**：您想要什么功能
2. **使用场景**：为什么需要这个功能
3. **建议实现**：（可选）如何实现这个功能

### 提交代码

#### 开发流程

1. **Fork 项目**

   ```bash
   # 在 GitHub 上 Fork 项目
   git clone https://github.com/YOUR_USERNAME/RemoteHelper.git
   cd RemoteHelper
   ```

2. **创建分支**

   ```bash
   git checkout -b feature/your-feature-name
   # 或
   git checkout -b fix/your-bug-fix
   ```

3. **开发与测试**

   ```bash
   # 编写代码
   # 运行测试
   cargo test --workspace
   
   # 代码检查
   cargo clippy --workspace -- -D warnings
   
   # 格式化
   cargo fmt --all
   ```

4. **提交更改**

   ```bash
   git add .
   git commit -m "feat: add new feature"
   # 或
   git commit -m "fix: resolve issue #123"
   ```

5. **推送并创建 PR**

   ```bash
   git push origin feature/your-feature-name
   # 在 GitHub 上创建 Pull Request
   ```

#### 提交信息规范

使用 [Conventional Commits](https://www.conventionalcommits.org/) 格式：

```
<type>(<scope>): <subject>

<body>

<footer>
```

**类型 (type)**:

- `feat`: 新功能
- `fix`: Bug 修复
- `docs`: 文档更新
- `style`: 代码格式（不影响功能）
- `refactor`: 重构（既不是新功能也不是 bug 修复）
- `perf`: 性能优化
- `test`: 添加或修改测试
- `chore`: 构建过程或辅助工具的变动

**示例**:

```
feat(server): add connection pooling support

Implement connection pooling to improve performance
when handling multiple concurrent clients.

Closes #42
```

## 📋 代码规范

### Rust 代码风格

1. **遵循 Rust 2024 Edition 惯用法**
2. **使用 `cargo fmt` 格式化代码**
3. **通过 `cargo clippy` 检查**（无警告）
4. **为公共 API 编写文档注释**

### 代码质量要求

- ✅ 所有测试必须通过
- ✅ Clippy 检查无警告
- ✅ 代码已格式化
- ✅ 新功能有单元测试
- ✅ 公共 API 有文档注释
- ✅ 复杂逻辑有内联注释

### 错误处理

- **服务器代码**：使用 `anyhow::Result`
- **客户端库**：使用 `Result<T, String>`
- **避免 panic**：使用 `?` 或 `.context()` 而不是 `.unwrap()`

### 并发安全

- 使用 `Arc<RwLock<T>>` 共享可变状态
- 使用 `AtomicUsize` 实现无锁计数器
- 在 `.await` 之前释放锁
- 按固定顺序获取锁（避免死锁）

## 🧪 测试指南

### 运行测试

```bash
# 运行所有测试
cargo test --workspace

# 运行特定包的测试
cargo test -p common
cargo test -p client
cargo test -p RemoteHelperServer

# 显示详细输出
cargo test -- --nocapture
```

### 编写测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_example() {
        assert_eq!(2 + 2, 4);
    }

    #[tokio::test]
    async fn test_async_example() {
        let result = async_function().await;
        assert!(result.is_ok());
    }
}
```

## 📝 文档贡献

### 更新文档

- **README.md**：用户文档和快速开始
- **copilot-instructions.md**：开发者指南和架构说明
- **CHANGELOG.md**：版本变更记录
- **代码注释**：复杂逻辑说明

### 文档风格

- 使用清晰、简洁的语言
- 提供代码示例
- 更新相关的配置示例
- 保持中英文文档同步

## 🔍 代码审查

Pull Request 将经过以下审查：

1. **功能正确性**：代码是否按预期工作
2. **代码质量**：是否符合项目规范
3. **测试覆盖**：是否有适当的测试
4. **文档完整性**：是否更新了相关文档
5. **向后兼容性**：是否破坏现有 API

## 🚀 发布流程

（仅限维护者）

1. 更新 `CHANGELOG.md`
2. 更新版本号（`Cargo.toml`）
3. 创建版本标签
4. 构建发布版本
5. 发布到 GitHub Releases

## 📞 联系方式

- **Issues**: [GitHub Issues](https://github.com/Hughsean/RemoteHelper/issues)
- **Email**: <Hughsean.F@outlook.com>

## 📜 行为准则

请遵守以下行为准则：

- 尊重所有贡献者
- 建设性的反馈
- 友好、包容的态度
- 专注于技术讨论
- 遵守开源精神

## 🙏 致谢

感谢所有贡献者的辛勤付出！您的每一个贡献都让项目变得更好。

---

**再次感谢您的贡献！** 🎉
