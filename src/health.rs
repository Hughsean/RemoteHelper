//! HTTP 健康检查工具模块。
//!
//! 用于在服务器启动前检测网络或依赖服务是否就绪。

use anyhow::Result;

/// 测试指定 URL 是否返回 503 状态码。
///
/// 用于判断目标服务（如 frp 隧道端点）是否可达但尚未准备好。
///
/// # 参数
/// * `url` - 要测试的 URL
/// * `timeout` - 超时秒数
///
/// # 返回值
/// * `Ok(true)` - 返回 503
/// * `Ok(false)` - 返回其他状态码
/// * `Err` - 请求失败
pub async fn test_http_503(url: &str, timeout: u64) -> Result<bool> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()?;

    let response = client.get(url).send().await?;
    let status = response.status();

    tracing::info!("URL: {}, 状态码: {}", url, status.as_u16());

    Ok(status == reqwest::StatusCode::SERVICE_UNAVAILABLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_frp_try_503() {
        let url = "http://frp-try.com:8880/";
        match test_http_503(url, 3).await {
            Ok(is_503) => {
                println!("测试结果: {} 返回 503: {}", url, is_503);
            }
            Err(e) => {
                eprintln!("请求失败: {}", e);
            }
        }
    }
}
