use crate::state::AppState;
use common::server_auth::{decode_base64, history_login_message};
use ed25519_dalek::{Signature, VerifyingKey};

/// 验证授权客户端公钥；历史协商成功时，签名还必须绑定握手摘要。
pub fn verify_login(
    state: &AppState,
    public_key: &str,
    signature: &str,
    challenge: &str,
    transcript_hash: Option<&[u8; 32]>,
) -> bool {
    if !state
        .config
        .web_panel
        .authorized_keys
        .iter()
        .any(|key| key == public_key)
    {
        return false;
    }
    let (Ok(key), Ok(signature), Ok(challenge)) = (
        decode_base64(public_key),
        decode_base64(signature),
        decode_base64(challenge),
    ) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&key) else {
        return false;
    };
    let extended;
    let message: &[u8] = match transcript_hash {
        Some(hash) => {
            extended = history_login_message(&challenge, hash);
            &extended
        }
        None => &challenge,
    };
    key.verify_strict(message, &Signature::from_bytes(&signature))
        .is_ok()
}
