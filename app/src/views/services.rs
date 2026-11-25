use crate::ui::{field, message, nav_variant, panel, request_error};
use common::{PathItem, Request, Response, ServiceAction, ServiceInfo};
use gpui_kit::assets::IconName;
use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::*;
use std::time::Duration;
use tokio::runtime::Handle;

pub(crate) struct Services {
    runtime: Handle,
    services: Option<Vec<ServiceInfo>>,
    error: Option<String>,
    busy: bool,
    form: Option<AddService>,
}

struct AddService {
    description: Entity<InputState>,
    path: Entity<InputState>,
    args: Entity<InputState>,
    suggestions: Vec<PathItem>,
    selected: usize,
    suggestion_scroll: ScrollHandle,
    query: u64,
    _path_events: Subscription,
}

impl Services {
    pub(crate) fn new(runtime: Handle, _: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            runtime,
            services: None,
            error: None,
            busy: false,
            form: None,
        }
    }

    pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.run(Request::ListServices, cx);
    }

    fn run(&mut self, request: Request, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();

        let task = self.runtime.spawn(update_services(request));
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(|error| format!("服务请求失败：{error}"));
            _ = this.update(cx, |view, cx| {
                view.busy = false;
                match result {
                    Ok(Ok((added, services))) => {
                        if added {
                            view.form = None;
                        }
                        match services {
                            Ok(services) => view.services = Some(services),
                            Err(error) => view.error = Some(error),
                        }
                    }
                    Ok(Err(error)) | Err(error) => view.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_form(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let description = cx.new(|cx| InputState::new(window, cx).placeholder("例如：备份任务"));
        let path =
            cx.new(|cx| InputState::new(window, cx).placeholder("远程主机上的可执行文件路径"));
        let args = cx.new(|cx| InputState::new(window, cx).placeholder("用空格分隔"));
        let path_events =
            cx.subscribe_in(
                &path,
                window,
                |view, state, event, window, cx| match event {
                    InputEvent::Change => {
                        view.suggest_paths(state.read(cx).value().to_string(), cx)
                    }
                    InputEvent::PressEnter { .. } => {
                        if let Some(form) = &view.form
                            && !form.suggestions.is_empty()
                        {
                            view.choose_suggestion(form.selected, window, cx);
                        }
                    }
                    _ => {}
                },
            );
        description.update(cx, |input, cx| input.focus(window, cx));
        self.error = None;
        self.form = Some(AddService {
            description,
            path,
            args,
            suggestions: Vec::new(),
            selected: 0,
            suggestion_scroll: ScrollHandle::new(),
            query: 0,
            _path_events: path_events,
        });
        cx.notify();
    }

    fn suggest_paths(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(form) = &mut self.form else { return };
        form.suggestions.clear();
        form.selected = 0;
        form.suggestion_scroll.set_offset(point(px(0.), px(0.)));
        form.query = form.query.wrapping_add(1);
        let query = form.query;
        let input = form.path.downgrade();
        cx.notify();
        if path.trim().is_empty() || self.busy {
            return;
        }
        let runtime = self.runtime.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let is_current = this.update(cx, |view, _| {
                !view.busy
                    && view
                        .form
                        .as_ref()
                        .is_some_and(|form| form.query == query && form.path.downgrade() == input)
            });
            if !matches!(is_current, Ok(true)) {
                return;
            }
            let result = runtime.spawn(client::query_path(path)).await;
            if let Ok(Ok(items)) = result {
                _ = this.update(cx, |view, cx| {
                    if let Some(form) = &mut view.form
                        && form.query == query
                        && form.path.downgrade() == input
                        && !view.busy
                    {
                        form.suggestions = items;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn on_path_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.form else { return };
        if self.busy
            || form.suggestions.is_empty()
            || !form.path.read(cx).focus_handle(cx).is_focused(window)
        {
            return;
        }
        match event.keystroke.key.as_str() {
            "up" => form.selected = form.selected.saturating_sub(1),
            "down" => form.selected = (form.selected + 1).min(form.suggestions.len() - 1),
            _ => return,
        }
        form.suggestion_scroll.scroll_to_item(form.selected);
        cx.stop_propagation();
        cx.notify();
    }
    fn choose_suggestion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.form else { return };
        let Some(item) = form.suggestions.get(index) else {
            return;
        };
        let mut value = item.path.clone();
        if item.is_dir && !value.ends_with(['/', '\\']) {
            value.push(if value.contains('\\') { '\\' } else { '/' });
        }
        let is_dir = item.is_dir;
        form.suggestions.clear();
        form.query = form.query.wrapping_add(1);
        form.path.update(cx, |input, cx| {
            input.set_value(value.clone(), window, cx);
            input.focus(window, cx);
        });
        if is_dir {
            self.suggest_paths(value, cx);
        }
        cx.notify();
    }

    fn add_service(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.form else { return };
        let description = form.description.read(cx).value().trim().to_string();
        let exe_path = form.path.read(cx).value().trim().to_string();
        if description.is_empty() || exe_path.is_empty() {
            self.error = Some("请输入服务描述和可执行文件路径".to_string());
            cx.notify();
            return;
        }
        let args = form
            .args
            .read(cx)
            .value()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        self.run(
            Request::AddService {
                description,
                exe_path,
                args,
            },
            cx,
        );
    }

    fn render_form(&self, form: &AddService, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let busy = self.busy;
        let suggestions = v_flex()
            .gap_1()
            .id("path-suggestions")
            .track_scroll(&form.suggestion_scroll)
            .max_h(px(154.))
            .overflow_y_scroll()
            .children(form.suggestions.iter().enumerate().map(|(index, item)| {
                let kind = if item.is_dir {
                    "目录"
                } else if item.is_executable {
                    "程序"
                } else {
                    "文件"
                };
                Button::new(("suggestion", index))
                    .small()
                    .w_full()
                    .disabled(busy)
                    .with_variant(nav_variant(index == form.selected, cx))
                    .label(format!("{kind}  {}", item.path))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.choose_suggestion(index, window, cx)
                    }))
            }));
        panel(cx)
            .capture_key_down(cx.listener(Self::on_path_key))
            .child(div().text_lg().font_semibold().child("添加服务"))
            .child(field("描述", Input::new(&form.description).disabled(busy)))
            .child(field(
                "可执行文件路径",
                Input::new(&form.path).disabled(busy),
            ))
            .children((!form.suggestions.is_empty()).then_some(suggestions))
            .child(field(
                "启动参数（可选）",
                Input::new(&form.args).disabled(busy),
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("路径和启动参数由远程主机使用。"),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel-add")
                            .label("取消")
                            .ghost()
                            .disabled(busy)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.form = None;
                                view.error = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("submit-add")
                            .label("添加")
                            .with_variant(nav_variant(true, cx))
                            .disabled(busy)
                            .on_click(cx.listener(Self::add_service)),
                    ),
            )
    }

    fn render_service(&self, service: &ServiceInfo, cx: &mut Context<Self>) -> AnyElement {
        let id = service.id;
        let running = service.running;
        let status = if running { "运行中" } else { "已停止" };
        let status_color = if running {
            cx.theme().success
        } else {
            cx.theme().muted_foreground
        };
        let controls = if running {
            [
                Some(("restart", "重启", ServiceAction::Restart)),
                Some(("stop", "停止", ServiceAction::Stop)),
            ]
        } else {
            [Some(("start", "启动", ServiceAction::Start)), None]
        };

        panel(cx)
            .id(("service", id))
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                v_flex()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .font_medium()
                            .child(service.description.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .text_color(status_color)
                                    .child(div().size_1p5().rounded_full().bg(status_color))
                                    .child(status),
                            )
                            .children(service.pid.map(|pid| {
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("PID {pid}"))
                            })),
                    ),
            )
            .child(
                h_flex()
                    .flex_shrink_0()
                    .gap_2()
                    .children(controls.into_iter().flatten().map(|(key, label, action)| {
                        Button::new((key, id))
                            .small()
                            .label(label)
                            .disabled(self.busy)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.run(
                                    Request::ControlService {
                                        id,
                                        action: action.clone(),
                                    },
                                    cx,
                                );
                            }))
                    })),
            )
            .into_any_element()
    }
}

impl Render for Services {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = match &self.services {
            Some(services) if !services.is_empty() => v_flex()
                .gap_2()
                .children(
                    services
                        .iter()
                        .map(|service| self.render_service(service, cx)),
                )
                .into_any_element(),
            Some(_) => panel(cx)
                .items_center()
                .py_9()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_lg()
                        .size(px(40.))
                        .bg(cx.theme().primary.opacity(0.12))
                        .text_color(cx.theme().primary)
                        .child(Icon::from(IconName::Server)),
                )
                .child(div().text_sm().font_semibold().child("暂无服务"))
                .child(message(
                    "使用右上角的「添加服务」配置远程主机上的程序。",
                    false,
                    cx,
                ))
                .into_any_element(),
            None if self.busy => message("正在加载服务列表…", false, cx).into_any_element(),
            None => message("暂无服务列表。", false, cx).into_any_element(),
        };
        div()
            .id("services-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .gap_3()
                    .p_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .gap_3()
                            .child(div().text_lg().font_semibold().child("服务管理"))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("refresh-services")
                                            .icon(Icon::from(IconName::RefreshCw))
                                            .label("刷新")
                                            .disabled(self.busy)
                                            .on_click(
                                                cx.listener(|view, _, _, cx| view.refresh(cx)),
                                            ),
                                    )
                                    .child(
                                        Button::new("add-service")
                                            .icon(Icon::from(IconName::Plus))
                                            .label("添加服务")
                                            .with_variant(nav_variant(true, cx))
                                            .disabled(self.busy || self.form.is_some())
                                            .on_click(cx.listener(Self::open_form)),
                                    ),
                            ),
                    )
                    .children(
                        self.error
                            .as_ref()
                            .map(|error| message(error.clone(), true, cx)),
                    )
                    .children(self.form.as_ref().map(|form| self.render_form(form, cx)))
                    .child(list),
            )
    }
}

async fn update_services(
    request: Request,
) -> Result<(bool, Result<Vec<ServiceInfo>, String>), String> {
    let added = match request {
        Request::ListServices => return Ok((false, list_services().await)),
        Request::AddService { .. } => match client::send_request(request).await? {
            Response::ServiceAdded(_) => true,
            response => return Err(request_error(response)),
        },
        _ => match client::send_request(request).await? {
            Response::Ok => false,
            response => return Err(request_error(response)),
        },
    };
    let services = list_services()
        .await
        .map_err(|error| format!("操作已完成，刷新服务列表失败：{error}"));
    Ok((added, services))
}

async fn list_services() -> Result<Vec<ServiceInfo>, String> {
    match client::send_request(Request::ListServices).await? {
        Response::Services(services) => Ok(services),
        response => Err(request_error(response)),
    }
}
