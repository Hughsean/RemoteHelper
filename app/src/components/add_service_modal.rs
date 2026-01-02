//! 添加服务模态框组件

use dioxus::prelude::*;
use crate::state::use_services_state;
use crate::backend;

// 根据基础路径计算相对显示路径
fn get_display_path(full_path: &str, base_path: &str) -> String {
    use std::path::Path;

    // 如果基础路径为空，只显示文件名
    if base_path.is_empty() {
        return Path::new(full_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(full_path)
            .to_string();
    }

    // 规范化路径分隔符
    let full_normalized = full_path.replace('/', "\\");
    let base_normalized = base_path.replace('/', "\\");

    // 确保基础路径以分隔符结尾（作为目录处理）
    let base_with_sep = if base_normalized.ends_with('\\') {
        base_normalized.clone()
    } else {
        format!("{}\\", base_normalized)
    };

    // 如果完整路径以基础路径开头，去掉基础路径前缀
    if full_normalized.starts_with(&base_with_sep) {
        return full_normalized[base_with_sep.len()..].to_string();
    }

    // 如果完整路径就是基础路径本身，只显示文件名
    if full_normalized == base_normalized {
        return Path::new(full_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(full_path)
            .to_string();
    }

    // 否则只显示文件名
    Path::new(full_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(full_path)
        .to_string()
}

// 截断文件名，中间显示省略号
fn truncate_middle(text: &str, max_len: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_len {
        return text.to_string();
    }

    let keep_len = (max_len.saturating_sub(3)) / 2;
    let chars: Vec<char> = text.chars().collect();

    let start: String = chars.iter().take(keep_len).collect();
    let end: String = chars.iter().skip(char_count - keep_len).collect();

    format!("{}...{}", start, end)
}

#[component]
pub fn AddServiceModal() -> Element {
    let mut services = use_services_state();

    let mut description = use_signal(String::new);
    let mut exe_path = use_signal(String::new);
    let mut args = use_signal(String::new);
    let mut suggestions = use_signal(Vec::<common::PathItem>::new);
    let mut show_suggestions = use_signal(|| false);
    let mut selected_index = use_signal(|| 0usize);
    let mut window_start = use_signal(|| 0usize);
    const WINDOW_SIZE: usize = 6;

    let handle_submit = move |evt: FormEvent| {
        evt.stop_propagation();
        evt.prevent_default();

        let desc = description();
        let path = exe_path();
        let args_str = args();

        // 验证必填字段
        if desc.trim().is_empty() || path.trim().is_empty() {
            tracing::warn!("验证失败: 描述或路径不能为空");
            return;
        }

        let args_vec: Vec<String> = args_str.split_whitespace().map(|s| s.to_string()).collect();

        spawn(async move {
            match backend::add_service(desc, path, args_vec).await {
                Ok(_) => {
                    // 刷新服务列表
                    if let Ok(new_services) = backend::list_services().await {
                        services.write().update_services(new_services);
                    }
                    services.write().hide_add_modal();
                }
                Err(e) => {
                    tracing::error!("添加服务失败: {}", e);
                }
            }
        });
    };

    rsx! {
        div { class: "modal-overlay",
            div { class: "modal-box",
                div { class: "modal-header",
                    h3 { class: "modal-title", "添加自定义服务" }
                    button {
                        class: "btn-close",
                        onclick: move |_| {
                            services.write().hide_add_modal();
                        },
                        svg {
                            class: "icon-md",
                            fill: "none",
                            stroke: "currentColor",
                            view_box: "0 0 24 24",
                            path {
                                stroke_linecap: "round",
                                stroke_linejoin: "round",
                                stroke_width: "2",
                                d: "M6 18L18 6M6 6l12 12",
                            }
                        }
                    }
                }
                form { onsubmit: handle_submit, class: "modal-form",
                    div { class: "form-group",
                        label { class: "form-label", "描述" }
                        input {
                            r#type: "text",
                            class: "form-input",
                            value: "{description}",
                            oninput: move |e| description.set(e.value()),
                        }
                    }
                    div {
                        class: "form-group autocomplete-container",
                        position: "relative",
                        label { class: "form-label", "可执行文件路径" }
                        input {
                            r#type: "text",
                            class: "form-input",
                            value: "{exe_path}",
                            onfocus: move |_| {
                                let value = exe_path();
                                selected_index.set(0);
                                window_start.set(0);

                                spawn(async move {
                                    match backend::query_path(value).await {
                                        Ok(items) => {
                                            suggestions.set(items.clone());
                                            show_suggestions.set(!items.is_empty());
                                        }
                                        Err(_) => {
                                            suggestions.set(Vec::new());
                                            show_suggestions.set(false);
                                        }
                                    }
                                });
                            },
                            oninput: move |e| {
                                let value = e.value();
                                exe_path.set(value.clone());
                                selected_index.set(0);
                                window_start.set(0);

                                spawn(async move {
                                    match backend::query_path(value).await {
                                        Ok(items) => {
                                            suggestions.set(items.clone());
                                            show_suggestions.set(!items.is_empty());
                                        }
                                        Err(_) => {
                                            suggestions.set(Vec::new());
                                            show_suggestions.set(false);
                                        }
                                    }
                                });
                            },
                            onkeydown: move |evt: Event<KeyboardData>| {
                                let key = evt.data.key();

                                if show_suggestions() && !suggestions().is_empty() {
                                    match key {
                                        Key::ArrowDown => {
                                            evt.stop_propagation();
                                            evt.prevent_default();
                                            let sugg_len = suggestions().len();
                                            let new_idx = (selected_index() + 1) % sugg_len;
                                            selected_index.set(new_idx);

                                            let current_window = window_start();
                                            if new_idx >= current_window + WINDOW_SIZE {
                                                window_start.set(new_idx - WINDOW_SIZE + 1);
                                            } else if new_idx < current_window {
                                                window_start.set(0);
                                            }
                                        }
                                        Key::ArrowUp => {
                                            evt.stop_propagation();
                                            evt.prevent_default();
                                            let sugg_len = suggestions().len();
                                            let idx = selected_index();
                                            let new_idx = if idx == 0 { sugg_len - 1 } else { idx - 1 };
                                            selected_index.set(new_idx);

                                            let current_window = window_start();
                                            if new_idx < current_window {
                                                window_start.set(new_idx);
                                            } else if new_idx >= current_window + WINDOW_SIZE {
                                                let max_start = sugg_len.saturating_sub(WINDOW_SIZE);
                                                window_start.set(max_start);
                                            }
                                        }
                                        Key::Enter => {
                                            evt.stop_propagation();
                                            evt.prevent_default();
                                            let sugg = suggestions();
                                            let selected = &sugg[selected_index()];
                                            let path = selected.path.clone();
                                            let is_dir = selected.is_dir;

                                            if is_dir {
                                                selected_index.set(0);
                                                window_start.set(0);

                                                spawn(async move {
                                                    match backend::query_path(path.clone()).await {
                                                        Ok(items) => {
                                                            exe_path.set(path);
                                                            suggestions.set(items.clone());
                                                            show_suggestions.set(!items.is_empty());
                                                        }
                                                        Err(_) => {
                                                            suggestions.set(Vec::new());
                                                            show_suggestions.set(false);
                                                        }
                                                    }
                                                });
                                            } else {
                                                exe_path.set(path);
                                                show_suggestions.set(false);
                                                suggestions.set(Vec::new());
                                            }
                                        }
                                        Key::Escape => {
                                            evt.stop_propagation();
                                            evt.prevent_default();
                                            show_suggestions.set(false);
                                        }
                                        _ => {}
                                    }
                                }
                            },
                            autocomplete: "off",
                        }
                        if show_suggestions() {
                            {
                                let current_selected = selected_index();
                                let current_window = window_start();
                                let all_suggestions = suggestions();
                                let window_end = (current_window + WINDOW_SIZE).min(all_suggestions.len());
                                let visible_items: Vec<_> = all_suggestions[current_window..window_end].to_vec();

                                rsx! {
                                    div {
                                        class: "autocomplete-suggestions-floating",
                                        position: "absolute",
                                        z_index: "1000",
                                        top: "100%",
                                        left: "0",
                                        right: "0",
                                        margin_top: "4px",
                                        background_color: "#1e293b",
                                        border: "1px solid #475569",
                                        border_radius: "8px",
                                        box_shadow: "0 10px 25px rgba(0, 0, 0, 0.5)",
                                        max_height: "320px",
                                        overflow_y: "auto",
                                        for (offset , item) in visible_items.iter().enumerate() {
                                            {
                                                let idx = current_window + offset;
                                                let path = item.path.clone();
                                                let is_dir = item.is_dir;
                                                let is_exe = item.is_executable;
                                                let is_selected = idx == current_selected;



                                                let current_input = exe_path();
                                                let relative_path = get_display_path(&path, &current_input);
                                                let display_name = truncate_middle(&relative_path, 50);

                                                rsx! {
                                                    div {
                                                        key: "{idx}",
                                                        class: "suggestion-item",
                                                        background_color: if is_selected { "#4f46e5" },
                                                        border_left: if is_selected { "3px solid #818cf8" },
                                                        onclick: move |_| {
                                                            if is_dir {
                                                                let path_clone = path.clone();
                                                                spawn(async move {
                                                                    match backend::query_path(path_clone.clone()).await {
                                                                        Ok(items) => {
                                                                            exe_path.set(path_clone);
                                                                            suggestions.set(items.clone());
                                                                            show_suggestions.set(!items.is_empty());
                                                                            selected_index.set(0);
                                                                            window_start.set(0);
                                                                        }
                                                                        Err(_) => {
                                                                            suggestions.set(Vec::new());
                                                                            show_suggestions.set(false);
                                                                        }
                                                                    }
                                                                });
                                                            } else {
                                                                exe_path.set(path.clone());
                                                                show_suggestions.set(false);
                                                                suggestions.set(Vec::new());
                                                            }
                                                        },
                                                        div { class: "suggestion-content",
                                                            if is_dir {
                                                                span { class: "suggestion-icon dir", "📁" }
                                                            } else if is_exe {
                                                                span { class: "suggestion-icon exe", "⚙️" }
                                                            } else {
                                                                span { class: "suggestion-icon file", "📄" }
                                                            }
                                                            span { class: "suggestion-path", "{display_name}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "form-group",
                        label { class: "form-label", "参数 (空格分隔)" }
                        input {
                            r#type: "text",
                            class: "form-input",
                            value: "{args}",
                            oninput: move |e| args.set(e.value()),
                        }
                    }
                    div { class: "modal-actions",
                        button {
                            r#type: "button",
                            class: "btn-cancel",
                            onclick: move |_| {
                                services.write().hide_add_modal();
                            },
                            "取消"
                        }
                        button { r#type: "submit", class: "btn-primary", "添加服务" }
                    }
                }
            }
        }
    }
}
