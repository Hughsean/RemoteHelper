use crate::handlers::Handler;
use crate::state::AppState;
use common::{PathItem, Request, Response};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use tokio::sync::mpsc;

pub struct PathHandler;

impl Handler for PathHandler {
    fn can_handle(&self, req: &Request) -> bool {
        matches!(req, Request::QueryPath { .. })
    }

    fn handle<'a>(
        &'a self,
        req: Request,
        _state: AppState,
        resp_tx: mpsc::Sender<Response>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let path = match req {
                Request::QueryPath { path } => path,
                _ => return,
            };
            let response = query_path_suggestions(&path);
            let _ = resp_tx.send(response).await;
        })
    }
}

fn query_path_suggestions(path: &str) -> Response {
    // 处理路径：如果是空的，使用当前目录
    let input_path = if path.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(path)
    };

    // 确定要搜索的目录和前缀
    let (search_dir, prefix) = if input_path.exists() && input_path.is_dir() {
        (input_path.clone(), String::new())
    } else {
        let parent = input_path.parent().unwrap_or(Path::new("."));
        let file_name = input_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        (parent.to_path_buf(), file_name)
    };

    // 读取目录并过滤
    let mut suggestions = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&search_dir) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                let file_name = entry.file_name();
                let name_str = file_name.to_string_lossy();

                if !prefix.is_empty()
                    && !name_str.to_lowercase().starts_with(&prefix.to_lowercase())
                {
                    continue;
                }

                let is_dir = metadata.is_dir();
                let full_path = entry.path();
                let path_str = full_path.to_string_lossy().to_string();

                let is_executable = if cfg!(windows) {
                    path_str.to_lowercase().ends_with(".exe")
                } else {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        metadata.permissions().mode() & 0o111 != 0
                    }
                    #[cfg(not(unix))]
                    false
                };

                suggestions.push(PathItem {
                    path: path_str,
                    is_dir,
                    is_executable,
                });
            }
        }
    }

    // 按类型和名称排序：目录优先，然后是可执行文件，最后是其他文件
    suggestions.sort_by(|a, b| {
        use std::cmp::Ordering;
        match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => match (a.is_executable, b.is_executable) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => a.path.to_lowercase().cmp(&b.path.to_lowercase()),
            },
        }
    });

    suggestions.truncate(50);
    Response::PathSuggestions(suggestions)
}
