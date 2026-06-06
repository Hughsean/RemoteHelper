use crate::handlers::Handler;
use crate::state::AppState;
use base64::prelude::*;
use common::{Request, Response};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;

pub struct AuthHandler;

impl Handler for AuthHandler {
    fn can_handle(&self, req: &Request) -> bool {
        matches!(req, Request::GetChallenge)
    }

    fn handle<'a>(
        &'a self,
        req: Request,
        _state: AppState,
        resp_tx: mpsc::Sender<Response>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if let Request::GetChallenge = req {
                let nonce: [u8; 32] = rand::random();
                let nonce_str = BASE64_STANDARD.encode(nonce);
                let _ = resp_tx.send(Response::Challenge(nonce_str)).await;
            }
        })
    }
}

/// 验证 Ed25519 签名。由连接层在验证 challenge 时调用。
pub fn verify_login(state: &AppState, pub_key_b64: &str, sig_b64: &str, challenge: &str) -> bool {
    tracing::debug!(
        "Verifying login for public key: {}...",
        &pub_key_b64.chars().take(16).collect::<String>()
    );

    // 检查密钥是否被授权
    if !state
        .config
        .web_panel
        .authorized_keys
        .contains(&pub_key_b64.to_string())
    {
        tracing::warn!("未授权的公钥尝试登录");
        return false;
    }

    let pub_key_bytes = match BASE64_STANDARD.decode(pub_key_b64) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let sig_bytes = match BASE64_STANDARD.decode(sig_b64) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let challenge_bytes = match BASE64_STANDARD.decode(challenge) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let verifying_key =
        match VerifyingKey::from_bytes(pub_key_bytes.as_slice().try_into().unwrap_or(&[0; 32])) {
            Ok(k) => k,
            Err(_) => return false,
        };

    let signature = match Signature::from_slice(&sig_bytes) {
        Ok(s) => s,
        Err(_) => {
            tracing::warn!("无效的签名格式");
            return false;
        }
    };

    let result = verifying_key.verify(&challenge_bytes, &signature).is_ok();
    if result {
        tracing::info!("登录验证成功");
    } else {
        tracing::warn!("登录验证失败 - 签名无效");
    }
    result
}
