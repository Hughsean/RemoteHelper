use crate::notifications::Notices;
use crate::ui::{field, nav_variant, panel, request_error};
use common::{PathItem, Request, Response, ServiceAction, ServiceInfo};
use gpui_kit::assets::IconName;
use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::{
    ActiveTheme, Icon, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    dialog::DialogEntrance,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::*;
use std::time::Duration;
use tokio::runtime::Handle;

pub(crate) struct Services {
    runtime: Handle,
    notices: Notices,
    services: Option<Vec<ServiceInfo>>,
    busy: bool,
}

impl Services {
    pub(crate) fn new(runtime: Handle, notices: Notices) -> Self {
        Self {
            runtime,
            notices,
            services: None,
            busy: false,
        }
    }

    pub(crate) fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run(Request::ListServices, None, window, cx);
    }

    fn run(
        &mut self,
        request: Request,
        form: Option<WeakEntity<AddService>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.busy {
            return false;
        }
        self.busy = true;
        cx.notify();

        let confirmation = match &request {
            Request::AddService { .. } => Some("已添加服务"),
            Request::ControlService { action, .. } => Some(match action {
                ServiceAction::Start => "服务已启动",
                ServiceAction::Stop => "服务已停止",
                ServiceAction::Restart => "服务已重启",
            }),
            _ => None,
        };
        let task = self.runtime.spawn(update_services(request));
        cx.spawn_in(window, async move |this, cx| {
            let result = task
                .await
                .map_err(|error| format!("服务请求失败：{error}"))
                .and_then(|result| result);
            let _ = this.update_in(cx, |view, window, cx| {
                view.busy = false;
                match result {
                    Ok((added, services)) => {
                        if added {
                            window.close_dialog(cx);
                        }
                        match services {
                            Ok(services) => {
                                view.services = Some(services);
                                if let Some(message) = confirmation {
                                    view.notices.success(message, cx);
                                }
                            }
                            Err(error) if confirmation.is_some() => view.notices.warning(error, cx),
                            Err(error) => {
                                view.notices.error(format!("无法刷新服务列表：{error}"), cx)
                            }
                        }
                    }
                    Err(error) => {
                        if let Some(form) = form {
                            let _ = form.update(cx, |form, cx| {
                                form.saving = false;
                                cx.notify();
                            });
                        }
                        view.notices.error(error, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        true
    }

    fn open_form(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || window.has_active_dialog(cx) {
            return;
        }
        let owner = cx.weak_entity();
        let runtime = self.runtime.clone();
        let notices = self.notices;
        let form = cx.new(|cx| AddService::new(owner, runtime, notices, window, cx));
        let first_field = form.read(cx).description.clone();
        window.open_dialog(cx, move |dialog, _, cx| {
            let saving = form.read(cx).saving;
            dialog
                .title("添加服务")
                .centered()
                .w(px(512.))
                .entrance(DialogEntrance::FadeSlide)
                .close_button(!saving)
                .keyboard(!saving)
                .overlay_closable(false)
                .child(form.clone())
        });
        first_field.update(cx, |input, cx| input.focus(window, cx));
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
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.run(
                                    Request::ControlService {
                                        id,
                                        action: action.clone(),
                                    },
                                    None,
                                    window,
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
            _ => panel(cx)
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
                .child(div().text_sm().font_semibold().child(match &self.services {
                    Some(_) => "暂无服务",
                    None if self.busy => "正在加载服务列表…",
                    None => "暂无服务列表",
                }))
                .into_any_element(),
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
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.refresh(window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("add-service")
                                            .icon(Icon::from(IconName::Plus))
                                            .label("添加服务")
                                            .primary()
                                            .disabled(self.busy)
                                            .on_click(cx.listener(Self::open_form)),
                                    ),
                            ),
                    )
                    .child(list),
            )
    }
}

struct AddService {
    services: WeakEntity<Services>,
    runtime: Handle,
    notices: Notices,
    description: Entity<InputState>,
    path: Entity<InputState>,
    args: Entity<InputState>,
    saving: bool,
    suggestions: Vec<PathItem>,
    selected: usize,
    suggestion_scroll: ScrollHandle,
    query: u64,
    _path_events: Subscription,
}

impl AddService {
    fn new(
        services: WeakEntity<Services>,
        runtime: Handle,
        notices: Notices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let description = cx.new(|cx| InputState::new(window, cx).placeholder("例如：备份任务"));
        let path =
            cx.new(|cx| InputState::new(window, cx).placeholder("远程主机上的可执行文件路径"));
        let args = cx.new(|cx| InputState::new(window, cx).placeholder("用空格分隔"));
        let path_events =
            cx.subscribe_in(
                &path,
                window,
                |form, input, event, window, cx| match event {
                    InputEvent::Change => {
                        form.suggest_paths(input.read(cx).value().to_string(), cx)
                    }
                    InputEvent::PressEnter { .. } if !form.suggestions.is_empty() => {
                        form.choose_suggestion(form.selected, window, cx);
                    }
                    _ => {}
                },
            );
        Self {
            services,
            runtime,
            notices,
            description,
            path,
            args,
            saving: false,
            suggestions: Vec::new(),
            selected: 0,
            suggestion_scroll: ScrollHandle::new(),
            query: 0,
            _path_events: path_events,
        }
    }

    fn suggest_paths(&mut self, path: String, cx: &mut Context<Self>) {
        self.suggestions.clear();
        self.selected = 0;
        self.suggestion_scroll.set_offset(point(px(0.), px(0.)));
        self.query = self.query.wrapping_add(1);
        let query = self.query;
        cx.notify();
        if path.trim().is_empty() || self.saving {
            return;
        }
        let runtime = self.runtime.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let current = this.update(cx, |form, _| !form.saving && form.query == query);
            if !matches!(current, Ok(true)) {
                return;
            }
            if let Ok(Ok(items)) = runtime.spawn(client::query_path(path)).await {
                let _ = this.update(cx, |form, cx| {
                    if !form.saving && form.query == query {
                        form.suggestions = items;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn on_path_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving
            || self.suggestions.is_empty()
            || !self.path.read(cx).focus_handle(cx).is_focused(window)
        {
            return;
        }
        match event.keystroke.key.as_str() {
            "up" => self.selected = self.selected.saturating_sub(1),
            "down" => self.selected = (self.selected + 1).min(self.suggestions.len() - 1),
            _ => return,
        }
        self.suggestion_scroll.scroll_to_item(self.selected);
        cx.stop_propagation();
        cx.notify();
    }

    fn choose_suggestion(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.suggestions.get(index) else {
            return;
        };
        let mut value = item.path.clone();
        let is_dir = item.is_dir;
        if is_dir && !value.ends_with(['/', '\\']) {
            value.push(if value.contains('\\') { '\\' } else { '/' });
        }
        self.suggestions.clear();
        self.query = self.query.wrapping_add(1);
        self.path.update(cx, |input, cx| {
            input.set_value(value.clone(), window, cx);
            input.focus(window, cx);
        });
        if is_dir {
            self.suggest_paths(value, cx);
        }
        cx.notify();
    }

    fn submit(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let description = self.description.read(cx).value().trim().to_string();
        let exe_path = self.path.read(cx).value().trim().to_string();
        if description.is_empty() || exe_path.is_empty() {
            self.notices.warning("请输入服务描述和可执行文件路径", cx);
            return;
        }
        let args = self
            .args
            .read(cx)
            .value()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let form = cx.weak_entity();
        self.saving = self
            .services
            .update(cx, |view, cx| {
                view.run(
                    Request::AddService {
                        description,
                        exe_path,
                        args,
                    },
                    Some(form),
                    window,
                    cx,
                )
            })
            .unwrap_or(false);
        cx.notify();
    }
}

impl Render for AddService {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.saving;
        let suggestions = v_flex()
            .gap_1()
            .id("path-suggestions")
            .track_scroll(&self.suggestion_scroll)
            .max_h(px(138.))
            .overflow_y_scroll()
            .children(self.suggestions.iter().enumerate().map(|(index, item)| {
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
                    .with_variant(nav_variant(index == self.selected, cx))
                    .label(format!("{kind}  {}", item.path))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.choose_suggestion(index, window, cx)
                    }))
            }));
        v_flex()
            .gap_4()
            .capture_key_down(cx.listener(Self::on_path_key))
            .child(
                v_flex()
                    .id("add-service-fields")
                    .gap_3()
                    .pr_1()
                    .max_h((window.viewport_size().height - px(160.)).max(px(150.)))
                    .overflow_y_scroll()
                    .child(field("描述", Input::new(&self.description).disabled(busy)))
                    .child(field(
                        "可执行文件路径",
                        Input::new(&self.path).disabled(busy),
                    ))
                    .children((!self.suggestions.is_empty()).then_some(suggestions))
                    .child(field(
                        "启动参数（可选）",
                        Input::new(&self.args).disabled(busy),
                    )),
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
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("submit-add")
                            .label(if busy { "正在添加…" } else { "添加" })
                            .primary()
                            .disabled(busy)
                            .on_click(cx.listener(Self::submit)),
                    ),
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
