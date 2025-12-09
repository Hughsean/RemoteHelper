use crate::config::Config;
use crate::logger::write_app_log;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use std::fs;

pub fn send_email(
    config: &Config,
    subject: &str,
    content: &str,
    is_error: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // 检查邮件通知是否启用
    if !config.smtp.enabled {
        write_app_log(&format!(
            "Email notification is disabled. Skipping email: '{}'",
            subject
        ));
        return Ok(());
    }

    write_app_log(&format!(
        "Preparing to send email. Subject: '{}', Is Error: {}",
        subject, is_error
    ));

    let color = if is_error { "#d9534f" } else { "#28a745" };
    let bg_color = if is_error { "#f9f2f4" } else { "#d4edda" };
    let text_color = if is_error { "#c7254e" } else { "#155724" };
    let intro_text = if is_error {
        "FrpcStartup 程序遇到意外错误并已终止。"
    } else {
        "FrpcStartup 程序已成功启动。"
    };

    let template = fs::read_to_string("email_template.html")
        .map_err(|e| format!("Failed to read email_template.html: {}", e))?;

    let html_body = template
        .replace("{{COLOR}}", color)
        .replace("{{TEXT_COLOR}}", text_color)
        .replace("{{BG_COLOR}}", bg_color)
        .replace("{{SUBJECT}}", subject)
        .replace("{{INTRO}}", intro_text)
        .replace("{{CONTENT}}", content);

    let email = Message::builder()
        .from(config.smtp.from.parse()?)
        .to(config.smtp.to.parse()?)
        .subject(subject)
        .header(ContentType::TEXT_HTML)
        .body(html_body)?;

    let creds = Credentials::new(config.smtp.username.clone(), config.smtp.password.clone());

    // 使用 relay 连接 SMTP 服务器
    let mailer = SmtpTransport::relay(&config.smtp.server)?
        .credentials(creds)
        .build();

    match mailer.send(&email) {
        Ok(_) => {
            write_app_log(&format!("Email sent successfully: '{}'", subject));
            Ok(())
        }
        Err(e) => {
            write_app_log(&format!("Failed to send email '{}': {}", subject, e));
            Err(e.into())
        }
    }
}
