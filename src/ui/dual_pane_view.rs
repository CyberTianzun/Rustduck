use crate::i18n::i18n;
use crate::models::{format_bytes, FileEntry, FileKind};
use crate::ui::file_pane::{
    parent_local_or_remote, split_breadcrumb_segments, SortColumn,
};
use crate::ui::native_file_list::{
    compute_pane_visibility, NativeFileListHost, NativePaneSnapshot, NativeRowViewModel,
};
use crate::ui::{
    accent_blue, bg_canvas, bg_elevated, bg_panel, border_subtle, is_dark_theme,
    protocol_icon_and_color, text_muted, text_primary, workspace::RustduckWorkspace,
};
use gpui::{
    div, prelude::*, px, relative, AnyElement, App, Bounds, Context, Element, ElementId, Entity,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window,
};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::resizable::{h_resizable, resizable_panel};
use gpui_component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit_assets::IconName;
use std::sync::Arc;

pub fn render_dual_pane_view(
    tab_idx: usize,
    ws: &RustduckWorkspace,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let top_toolbar = render_connected_session_toolbar(tab_idx, ws, cx);
    let left_pane = render_single_pane(tab_idx, true, ws, cx);
    let right_pane = render_single_pane(tab_idx, false, ws, cx);

    v_flex()
        .size_full()
        .bg(bg_canvas())
        .child(top_toolbar)
        .child(
            div()
                .flex_1()
                .w_full()
                .overflow_hidden()
                .child(
                    h_resizable(("transmit-dual-pane-split", tab_idx))
                        .child(resizable_panel().child(left_pane))
                        .child(resizable_panel().child(right_pane)),
                ),
        )
        .into_any_element()
}

fn render_connected_session_toolbar(
    tab_idx: usize,
    ws: &RustduckWorkspace,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let tab = &ws.tabs[tab_idx];
    let show_hidden = tab.show_hidden;
    let (proto_icon, proto_color) = protocol_icon_and_color(tab.form.protocol);

    h_flex()
        .w_full()
        .h(px(36.0))
        .px_4()
        .bg(bg_panel())
        .border_b_1()
        .border_color(border_subtle())
        .justify_between()
        .items_center()
        .child(
            h_flex()
                .gap_2p5()
                .items_center()
                .child(
                    Button::new("back-to-bookmarks")
                        .xsmall()
                        .secondary()
                        .icon(IconName::Star)
                        .label(t.favorites_back_btn())
                        .tooltip(t.favorites_back_tooltip())
                        .on_click(cx.listener(move |this, _ev, _win, cx| {
                            this.disconnect_tab(tab_idx, cx);
                        })),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .px_2p5()
                        .py_0p5()
                        .rounded(px(6.0))
                        .bg(bg_elevated())
                        .border_1()
                        .border_color(border_subtle())
                        .items_center()
                        .child(Icon::new(proto_icon).xsmall().text_color(proto_color))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(text_primary())
                                .child(tab.title.clone()),
                        ),
                )
                .child(
                    Button::new("sync-refresh-both")
                        .xsmall()
                        .ghost()
                        .icon(IconName::RefreshCw)
                        .label(t.sync_refresh_both_btn())
                        .on_click(cx.listener(move |this, _ev, window, cx| {
                            this.refresh_local_pane(tab_idx, window, cx);
                            this.refresh_remote_pane(tab_idx, window, cx);
                        })),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    Button::new("toggle-hidden-files")
                        .xsmall()
                        .icon(if show_hidden {
                            IconName::Eye
                        } else {
                            IconName::EyeOff
                        })
                        .with_variant(if show_hidden {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Secondary
                        })
                        .label(t.toggle_hidden_btn(show_hidden))
                        .on_click(cx.listener(move |this, _ev, _win, cx| {
                            this.toggle_show_hidden(tab_idx, cx);
                        })),
                )
                .child(
                    Button::new("disconnect-session")
                        .xsmall()
                        .danger()
                        .icon(IconName::Unplug)
                        .label(t.disconnect_btn())
                        .on_click(cx.listener(move |this, _ev, _win, cx| {
                            this.disconnect_tab(tab_idx, cx);
                        })),
                ),
        )
        .into_any_element()
}

fn render_single_pane(
    tab_idx: usize,
    is_local: bool,
    ws: &RustduckWorkspace,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let tab = &ws.tabs[tab_idx];
    let current_path = if is_local {
        &tab.local_path
    } else {
        &tab.remote_path
    };
    let filter_input = if is_local {
        &tab.local_filter_input
    } else {
        &tab.remote_filter_input
    };
    let path_input = if is_local {
        &tab.local_path_input
    } else {
        &tab.remote_path_input
    };
    let is_editing_path = if is_local {
        tab.local_path_editing
    } else {
        tab.remote_path_editing
    };
    let (entries, rows, (dir_count, file_count, total_bytes), entries_revision, selected_paths) =
        if is_local {
            (
                Arc::clone(&tab.cached_local_visible),
                Arc::clone(&tab.cached_local_rows),
                tab.cached_local_summary,
                tab.local_entries_revision,
                tab.selected_local_paths.clone(),
            )
        } else {
            (
                Arc::clone(&tab.cached_remote_visible),
                Arc::clone(&tab.cached_remote_rows),
                tab.cached_remote_summary,
                tab.remote_entries_revision,
                tab.selected_remote_paths.clone(),
            )
        };

    let selected_entries = ws.selected_entries(tab_idx, is_local);
    let breadcrumbs = split_breadcrumb_segments(current_path, is_local);
    let has_parent = parent_local_or_remote(current_path, is_local).is_some();

    let action_header = render_pane_action_header(
        tab_idx,
        is_local,
        &tab.title,
        tab.form.protocol,
        filter_input,
        &selected_entries,
        cx,
    );
    let breadcrumb_bar = render_pane_breadcrumb_bar(
        tab_idx,
        is_local,
        is_editing_path,
        path_input,
        breadcrumbs,
        cx,
    );
    let file_table = render_pane_file_table(
        tab_idx,
        is_local,
        ws,
        current_path.clone(),
        has_parent,
        entries_revision,
        rows,
        entries,
        selected_paths,
        cx,
    );
    let footer = render_pane_footer(
        dir_count,
        file_count,
        total_bytes,
        selected_entries.len(),
        selected_entries.first().map(|e| e.name.as_str()),
    );

    v_flex()
        .size_full()
        .bg(bg_canvas())
        .child(action_header)
        .child(breadcrumb_bar)
        .child(file_table)
        .child(footer)
        .into_any_element()
}

fn render_pane_action_header(
    tab_idx: usize,
    is_local: bool,
    tab_title: &str,
    protocol: crate::models::ProtocolKind,
    filter_input: &Entity<InputState>,
    selected_entries: &[FileEntry],
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let side_prefix = if is_local { "local" } else { "remote" };
    let (pane_icon, pane_icon_color, pane_title) = if is_local {
        (
            IconName::Laptop,
            accent_blue(),
            t.local_pane_title().to_string(),
        )
    } else {
        let (p_icon, p_col) = protocol_icon_and_color(protocol);
        (p_icon, p_col, t.remote_pane_title(tab_title))
    };

    let can_transfer = selected_entries
        .iter()
        .any(|e| !matches!(e.kind, FileKind::Bucket));
    let single_for_rename = if selected_entries.len() == 1
        && !matches!(selected_entries[0].kind, FileKind::Bucket)
    {
        Some(selected_entries[0].clone())
    } else {
        None
    };
    let can_delete = can_transfer;

    h_flex()
        .w_full()
        .h(px(38.0))
        .px_3()
        .bg(bg_panel())
        .border_b_1()
        .border_color(border_subtle())
        .justify_between()
        .items_center()
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(Icon::new(pane_icon).xsmall().text_color(pane_icon_color))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(text_primary())
                        .child(pane_title),
                )
                .child(
                    Button::new((side_prefix, 1usize))
                        .xsmall()
                        .ghost()
                        .icon(IconName::ChevronUp)
                        .tooltip(t.nav_up_tooltip())
                        .on_click(cx.listener(move |this, _ev, window, cx| {
                            this.navigate_up(tab_idx, is_local, window, cx);
                        })),
                )
                .child(
                    Button::new((side_prefix, 2usize))
                        .xsmall()
                        .ghost()
                        .icon(IconName::RefreshCw)
                        .tooltip(t.refresh_dir_tooltip())
                        .on_click(cx.listener(move |this, _ev, window, cx| {
                            if is_local {
                                this.refresh_local_pane(tab_idx, window, cx);
                            } else {
                                this.refresh_remote_pane(tab_idx, window, cx);
                            }
                        })),
                ),
        )
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .when(can_transfer, |bar| {
                    bar.child(
                        Button::new((side_prefix, 3usize))
                            .xsmall()
                            .primary()
                            .icon(if is_local {
                                IconName::Upload
                            } else {
                                IconName::Download
                            })
                            .label(if is_local {
                                t.upload_btn()
                            } else {
                                t.download_btn()
                            })
                            .tooltip(if is_local {
                                t.upload_selected_tooltip()
                            } else {
                                t.download_selected_tooltip()
                            })
                            .on_click(cx.listener(move |this, _ev, _win, cx| {
                                this.enqueue_selected_transfers(tab_idx, is_local, cx);
                            })),
                    )
                })
                .child(
                    Button::new((side_prefix, 4usize))
                        .xsmall()
                        .secondary()
                        .icon(IconName::FolderPlus)
                        .tooltip(t.new_folder_tooltip())
                        .on_click(cx.listener(move |this, _ev, window, cx| {
                            this.open_new_folder_prompt(tab_idx, is_local, window, cx);
                        })),
                )
                .when_some(single_for_rename, |bar, sel_entry| {
                    bar.child(
                        Button::new((side_prefix, 5usize))
                            .xsmall()
                            .secondary()
                            .icon(IconName::Pencil)
                            .tooltip(t.rename_selected_tooltip())
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                this.open_rename_prompt(tab_idx, is_local, &sel_entry, window, cx);
                            })),
                    )
                })
                .when(can_delete, |bar| {
                    bar.child(
                        Button::new((side_prefix, 6usize))
                            .xsmall()
                            .danger()
                            .icon(IconName::Trash)
                            .tooltip(t.delete_selected_tooltip())
                            .on_click(cx.listener(move |this, _ev, _win, cx| {
                                this.open_delete_selected_confirm(tab_idx, is_local, cx);
                            })),
                    )
                })
                .child(
                    div().w(px(145.0)).child(
                        Input::new(filter_input)
                            .xsmall()
                            .prefix(Icon::new(IconName::Search).xsmall().text_color(text_muted()))
                            .cleanable(true),
                    ),
                ),
        )
        .into_any_element()
}

fn render_pane_breadcrumb_bar(
    tab_idx: usize,
    is_local: bool,
    is_editing_path: bool,
    path_input: &Entity<InputState>,
    breadcrumbs: Vec<(String, String)>,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let side_prefix = if is_local { "local" } else { "remote" };

    let content = if is_editing_path {
        h_flex()
            .flex_1()
            .gap_1p5()
            .items_center()
            .child(div().flex_1().child(Input::new(path_input).xsmall()))
            .child(
                Button::new((side_prefix, 10usize))
                    .xsmall()
                    .primary()
                    .label(t.go_path_btn())
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        let target = if is_local {
                            this.tabs[tab_idx]
                                .local_path_input
                                .read(cx)
                                .value()
                                .to_string()
                        } else {
                            this.tabs[tab_idx]
                                .remote_path_input
                                .read(cx)
                                .value()
                                .to_string()
                        };
                        this.jump_to_path(tab_idx, is_local, target, window, cx);
                    })),
            )
            .child(
                Button::new((side_prefix, 11usize))
                    .xsmall()
                    .ghost()
                    .icon(IconName::X)
                    .on_click(cx.listener(move |this, _ev, _win, cx| {
                        if is_local {
                            this.tabs[tab_idx].local_path_editing = false;
                        } else {
                            this.tabs[tab_idx].remote_path_editing = false;
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    } else {
        let items: Vec<AnyElement> = breadcrumbs
            .into_iter()
            .enumerate()
            .map(|(c_idx, (label, target_path))| {
                h_flex()
                    .gap_0p5()
                    .items_center()
                    .when(c_idx > 0, |el| {
                        el.child(
                            Icon::new(IconName::ChevronRight)
                                .xsmall()
                                .text_color(text_muted()),
                        )
                    })
                    .child(
                        Button::new((side_prefix, 100 + c_idx))
                            .xsmall()
                            .ghost()
                            .compact()
                            .label(label)
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                this.jump_to_path(
                                    tab_idx,
                                    is_local,
                                    target_path.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .into_any_element()
            })
            .collect();

        h_flex()
            .flex_1()
            .gap_0p5()
            .items_center()
            .overflow_hidden()
            .children(items)
            .into_any_element()
    };

    h_flex()
        .w_full()
        .h(px(32.0))
        .px_3()
        .gap_1()
        .bg(bg_elevated())
        .border_b_1()
        .border_color(border_subtle())
        .justify_between()
        .items_center()
        .child(content)
        .when(!is_editing_path, |bar| {
            bar.child(
                Button::new((side_prefix, 12usize))
                    .xsmall()
                    .ghost()
                    .compact()
                    .icon(IconName::Pencil)
                    .tooltip(t.edit_path_tooltip())
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        let tab = &mut this.tabs[tab_idx];
                        if is_local {
                            tab.local_path_editing = true;
                            let path_val = tab.local_path.clone();
                            tab.local_path_input.update(cx, |s, cx| {
                                s.set_value(path_val, window, cx);
                                s.focus(window, cx);
                            });
                        } else {
                            tab.remote_path_editing = true;
                            let path_val = tab.remote_path.clone();
                            tab.remote_path_input.update(cx, |s, cx| {
                                s.set_value(path_val, window, cx);
                                s.focus(window, cx);
                            });
                        }
                        cx.notify();
                    })),
            )
        })
        .into_any_element()
}

pub struct NativeFileListElement {
    host: Arc<NativeFileListHost>,
    ws_entity: Entity<RustduckWorkspace>,
    tab_idx: usize,
    is_local: bool,
    is_connected: bool,
    settings_open: bool,
    current_path: String,
    is_dark: bool,
    sort_col: SortColumn,
    sort_ascending: bool,
    has_parent: bool,
    entries_revision: u64,
    rows: Arc<Vec<NativeRowViewModel>>,
    entries: Arc<Vec<FileEntry>>,
    selected_paths: Vec<String>,
}

impl IntoElement for NativeFileListElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for NativeFileListElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::from((
            if self.is_local {
                "native-file-list-local"
            } else {
                "native-file-list-remote"
            },
            self.tab_idx,
        )))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let scale = window.scale_factor().max(0.5);
        let x = (f32::from(bounds.origin.x) * scale).round() as i32;
        let y = (f32::from(bounds.origin.y) * scale).round() as i32;
        let w = (f32::from(bounds.size.width) * scale).round() as i32;
        let h = (f32::from(bounds.size.height) * scale).round() as i32;
        let visible = compute_pane_visibility(self.is_connected, self.settings_open, w, h);

        let snapshot = NativePaneSnapshot {
            tab_idx: self.tab_idx,
            is_local: self.is_local,
            current_path: self.current_path.clone(),
            bounds_px: (x, y, w, h),
            scale_factor: scale,
            visible,
            is_dark: self.is_dark,
            sort_col: self.sort_col,
            sort_ascending: self.sort_ascending,
            has_parent: self.has_parent,
            entries_revision: self.entries_revision,
            rows: Arc::clone(&self.rows),
            entries: Arc::clone(&self.entries),
            selected_paths: self.selected_paths.clone(),
        };
        self.host.sync_pane(window, &snapshot);

        let events = self.host.drain_events();
        if !events.is_empty() {
            let ws_entity = self.ws_entity.clone();
            let tab_idx = self.tab_idx;
            window.defer(cx, move |window, cx| {
                ws_entity.update(cx, |ws, cx| {
                    ws.handle_native_pane_events(tab_idx, events, window, cx);
                });
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_pane_file_table(
    tab_idx: usize,
    is_local: bool,
    ws: &RustduckWorkspace,
    current_path: String,
    has_parent: bool,
    entries_revision: u64,
    rows: Arc<Vec<NativeRowViewModel>>,
    entries: Arc<Vec<FileEntry>>,
    selected_paths: Vec<String>,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let tab = &ws.tabs[tab_idx];
    let element = NativeFileListElement {
        host: Arc::clone(&ws.native_file_list_host),
        ws_entity: cx.entity(),
        tab_idx,
        is_local,
        is_connected: tab.is_connected,
        settings_open: ws.settings_open || tab.has_action_modal(),
        current_path,
        is_dark: is_dark_theme(),
        sort_col: tab.sort_column,
        sort_ascending: tab.sort_ascending,
        has_parent,
        entries_revision,
        rows,
        entries,
        selected_paths,
    };

    div()
        .flex_1()
        .w_full()
        .overflow_hidden()
        .child(element)
        .into_any_element()
}

fn render_pane_footer(
    dir_count: usize,
    file_count: usize,
    total_bytes: u64,
    selected_count: usize,
    primary_selected_name: Option<&str>,
) -> AnyElement {
    let t = i18n();
    h_flex()
        .w_full()
        .h(px(26.0))
        .px_3()
        .bg(bg_panel())
        .border_t_1()
        .border_color(border_subtle())
        .justify_between()
        .items_center()
        .child(
            div()
                .text_xs()
                .text_color(text_muted())
                .child(t.pane_footer_summary(dir_count, file_count, &format_bytes(total_bytes))),
        )
        .child(
            div()
                .text_xs()
                .text_color(if selected_count > 0 {
                    accent_blue()
                } else {
                    text_muted()
                })
                .child(t.pane_footer_multi_selection(selected_count, primary_selected_name)),
        )
        .into_any_element()
}
