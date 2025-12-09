use chrono::Local;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Mutex;

// 全局日志锁，防止多线程写入冲突
static LOG_MUTEX: Mutex<()> = Mutex::new(());

#[track_caller]
pub fn write_app_log(message: &str) {
    // 使用锁保护日志写入，防止多线程交错
    let _lock = LOG_MUTEX.lock().unwrap();

    let location = std::panic::Location::caller();
    let now = Local::now();
    let log_dir = "logs";
    if let Err(e) = fs::create_dir_all(log_dir) {
        eprintln!("Failed to create log directory: {}", e);
        return;
    }

    let log_path = format!("{}/application.log", log_dir);
    let mut file = match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open application log: {}", e);
            return;
        }
    };

    if let Err(e) = writeln!(
        file,
        "[{}] [{}:{}] {}",
        now.format("%Y-%m-%d %H:%M:%S"),
        location.file(),
        location.line(),
        message
    ) {
        eprintln!("Failed to write to application log: {}", e);
    }
    // 锁在此处自动释放
}

#[allow(dead_code)]
pub fn read_last_log(file_path: &str, max_bytes: u64) -> String {
    let mut file = match fs::File::open(file_path) {
        Ok(f) => f,
        Err(_) => return "无法读取日志文件".to_string(),
    };

    let metadata = match file.metadata() {
        Ok(m) => m,
        Err(_) => return "无法获取日志文件元数据".to_string(),
    };

    let len = metadata.len();
    let start = if len > max_bytes { len - max_bytes } else { 0 };

    if let Err(_) = file.seek(SeekFrom::Start(start)) {
        return "无法定位日志文件读取位置".to_string();
    }

    let mut buffer = Vec::new();
    if let Err(_) = file.read_to_end(&mut buffer) {
        return "读取日志文件失败".to_string();
    }

    String::from_utf8_lossy(&buffer).to_string()
}
