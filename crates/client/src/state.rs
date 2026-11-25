//! 全局状态模块 —— 客户端生命周期的可变量集中管理。
//!
//! 四个全局静态变量覆盖了从密钥到连接的完整客户端状态：
//! - `SIGNING_KEY`: 解密后的 Ed25519 签名密钥与对应公钥
//! - `SERVER_ADDRESS`: 目标服务器地址
//! - `CONNECTION`: 当前已认证的加密连接
//! - `MOCK_MODE`: 离线模拟开关

use crate::EncryptedConnection;
use ed25519_dalek::SigningKey;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

/// 解密后的 Ed25519 签名密钥与其 Base64 公钥。
///
/// 在登录页加载密钥文件并解密后填入。若为 `None` 则表示尚未认证。
pub static SIGNING_KEY: LazyLock<Mutex<Option<(SigningKey, String)>>> =
    LazyLock::new(|| Mutex::new(None));

/// 目标服务器地址，格式为 `host:port`。
///
/// 默认连接 `frp-egg.com:28450`，可在登录页修改。
pub static SERVER_ADDRESS: LazyLock<Mutex<String>> =
    LazyLock::new(|| Mutex::new("frp-egg.com:28450".to_string()));

/// 当前已认证的加密连接。
///
/// 请求过程中若发现连接已断开则自动重建。
pub static CONNECTION: LazyLock<tokio::sync::Mutex<Option<EncryptedConnection>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));

/// Mock 模式开关 —— 启用后所有请求返回合成数据，不连接服务器。
pub static MOCK_MODE: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));
