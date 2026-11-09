use crate::i18n::i18n;
use crate::models::{format_bytes, TransferDirection, TransferStatus, TransferTask};
use crate::ui::{
    accent_amber, accent_blue, accent_green, accent_red, bg_canvas, bg_elevated, bg_panel,
    bg_row_alt, border_subtle, text_muted, text_primary, workspace::RustduckWorkspace,
};
use gpui::{div, prelude::*, px, relative, AnyElement, Context, IntoElement};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit_assets::IconName;

pub fn render_transfer_drawer(
    tasks: &[TransferTask],
    expanded: bool,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let header = render_drawer_header(tasks, expanded, cx);

    v_flex()
        .w_full()
        .bg(bg_panel())
        .border_t_1()
        .border_color(border_subtle())
        .child(header)
        .when(expanded, |drawer| {
            if tasks.is_empty() {
                drawer.child(
                    h_flex()
                        .w_full()
                        .h(px(64.0))
                        .px_4()
                        .bg(bg_canvas())
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            Icon::new(IconName::ArrowUpDown)
                                .small()
                                .text_color(text_muted()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(text_muted())
                                .child(t.drawer_empty_hint()),
                        ),
                )
            } else {
                let rows: Vec<AnyElement> = tasks
                    .iter()
                    .enumerate()
                    .map(|(idx, task)| render_transfer_task_row(idx, task, cx))
                    .collect();

                drawer.child(
                    v_flex()
                        .id("transfer-drawer-scroll-list")
                        .w_full()
                        .max_h(px(190.0))
                        .bg(bg_canvas())
                        .overflow_y_scrollbar()
                        .children(rows),
                )
            }
        })
        .into_any_element()
}

fn render_drawer_header(
    tasks: &[TransferTask],
    expanded: bool,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let active_count = tasks
        .iter()
        .filter(|t| matches!(t.status, TransferStatus::InProgress | TransferStatus::Queued))
        .count();
    let completed_count = tasks
        .iter()
        .filter(|t| matches!(t.status, TransferStatus::Completed))
        .count();
    let failed_count = tasks
        .iter()
        .filter(|t| matches!(t.status, TransferStatus::Failed(_)))
        .count();
    let total_speed: u64 = tasks
        .iter()
        .filter(|t| matches!(t.status, TransferStatus::InProgress))
        .map(|t| t.speed_bytes_per_sec)
        .sum();

    h_flex()
        .w_full()
        .h(px(32.0))
        .px_4()
        .justify_between()
        .items_center()
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(
                            Icon::new(IconName::ArrowUpDown)
                                .xsmall()
                                .text_color(if active_count > 0 {
                                    accent_blue()
                                } else {
                                    text_muted()
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(text_primary())
                                .child(t.drawer_title()),
                        ),
                )
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded(px(99.0))
                        .bg(bg_elevated())
                        .text_xs()
                        .text_color(if active_count > 0 {
                            accent_blue()
                        } else {
                            text_muted()
                        })
                        .child(t.drawer_active_badge(active_count)),
                )
                .child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded(px(99.0))
                        .bg(bg_elevated())
                        .text_xs()
                        .text_color(if completed_count > 0 {
                            accent_green()
                        } else {
                            text_muted()
                        })
                        .child(t.drawer_completed_badge(completed_count)),
                )
                .when(failed_count > 0, |bar| {
                    bar.child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded(px(99.0))
                            .bg(bg_elevated())
                            .text_xs()
                            .text_color(accent_red())
                            .child(t.drawer_failed_badge(failed_count)),
                    )
                })
                .when(total_speed > 0, |bar| {
                    bar.child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(accent_blue())
                            .child(format!("⚡ {}/s", format_bytes(total_speed))),
                    )
                }),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    Button::new("clear-completed-transfers")
                        .xsmall()
                        .ghost()
                        .icon(IconName::Trash)
                        .label(t.clear_completed_btn())
                        .on_click(cx.listener(|this, _ev, _win, cx| {
                            this.transfer_manager.clear_completed();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("toggle-transfer-drawer")
                        .xsmall()
                        .secondary()
                        .icon(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronUp
                        })
                        .label(t.toggle_drawer_btn(expanded))
                        .on_click(cx.listener(|this, _ev, _win, cx| {
                            this.drawer_expanded = !this.drawer_expanded;
                            cx.notify();
                        })),
                ),
        )
        .into_any_element()
}

fn render_transfer_task_row(
    idx: usize,
    task: &TransferTask,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let task_id = task.id;
    let pct = task.progress_percent();
    let ratio = (pct as f32 / 100.0).clamp(0.0, 1.0);
    let is_upload = task.direction == TransferDirection::Upload;

    let (status_text, status_color, bar_color) = match &task.status {
        TransferStatus::Queued => (t.status_queued(), text_muted(), accent_blue()),
        TransferStatus::InProgress => (
            format!("{}/s · {pct}%", format_bytes(task.speed_bytes_per_sec)),
            accent_blue(),
            accent_blue(),
        ),
        TransferStatus::Paused => (t.status_paused(), accent_amber(), accent_amber()),
        TransferStatus::Completed => (t.status_completed(), accent_green(), accent_green()),
        TransferStatus::Failed(err) => (t.status_failed(err), accent_red(), accent_red()),
        TransferStatus::Cancelled => (t.status_cancelled(), text_muted(), text_muted()),
    };

    h_flex()
        .w_full()
        .h(px(36.0))
        .px_4()
        .gap_3()
        .bg(if idx % 2 == 1 {
            bg_row_alt()
        } else {
            bg_canvas()
        })
        .border_b_1()
        .border_color(border_subtle())
        .justify_between()
        .items_center()
        .child(
            h_flex()
                .w(px(320.0))
                .gap_2()
                .items_center()
                .overflow_hidden()
                .child(
                    Icon::new(if is_upload {
                        IconName::Upload
                    } else {
                        IconName::Download
                    })
                    .xsmall()
                    .text_color(if is_upload {
                        accent_blue()
                    } else {
                        accent_green()
                    }),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(text_primary())
                        .child(task.file_name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_muted())
                        .child(format!(
                            "({} / {})",
                            format_bytes(task.transferred_bytes),
                            format_bytes(task.total_bytes)
                        )),
                ),
        )
        .child(
            div()
                .flex_1()
                .h(px(6.0))
                .rounded(px(99.0))
                .bg(bg_elevated())
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(ratio))
                        .rounded(px(99.0))
                        .bg(bar_color),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .w(px(150.0))
                        .flex()
                        .justify_end()
                        .text_xs()
                        .text_color(status_color)
                        .child(status_text),
                )
                .when(
                    matches!(
                        task.status,
                        TransferStatus::InProgress | TransferStatus::Queued
                    ),
                    |row| {
                        row.child(
                            Button::new(("pause-task", idx))
                                .xsmall()
                                .ghost()
                                .compact()
                                .icon(IconName::Pause)
                                .tooltip(t.pause_transfer_tooltip())
                                .on_click(cx.listener(move |this, _ev, _win, cx| {
                                    this.transfer_manager.pause(task_id);
                                    cx.notify();
                                })),
                        )
                    },
                )
                .when(
                    matches!(
                        task.status,
                        TransferStatus::Paused | TransferStatus::Failed(_)
                    ),
                    |row| {
                        row.child(
                            Button::new(("resume-task", idx))
                                .xsmall()
                                .ghost()
                                .compact()
                                .icon(IconName::Play)
                                .tooltip(t.resume_transfer_tooltip())
                                .on_click(cx.listener(move |this, _ev, _win, cx| {
                                    this.transfer_manager.resume(task_id);
                                    cx.notify();
                                })),
                        )
                    },
                )
                .when(
                    !matches!(
                        task.status,
                        TransferStatus::Completed | TransferStatus::Cancelled
                    ),
                    |row| {
                        row.child(
                            Button::new(("cancel-task", idx))
                                .xsmall()
                                .ghost()
                                .compact()
                                .icon(IconName::X)
                                .tooltip(t.cancel_transfer_tooltip())
                                .on_click(cx.listener(move |this, _ev, _win, cx| {
                                    this.transfer_manager.cancel(task_id);
                                    cx.notify();
                                })),
                        )
                    },
                ),
        )
        .into_any_element()
}
