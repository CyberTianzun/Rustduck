use crate::i18n::i18n;
use crate::models::{format_bytes, FileEntry, FileKind};
use crate::ui::file_pane::{
    filter_and_sort_entries, format_modified_time, parent_local_or_remote,
    split_breadcrumb_segments, summarize_pane_entries, PaneInlineAction, SortColumn,
};
use crate::ui::{
    accent_amber, accent_blue, accent_red, bg_canvas, bg_elevated, bg_panel, bg_row_alt,
    bg_row_hover, bg_row_selected, border_selected, border_subtle, entry_icon_and_color,
    protocol_icon_and_color, text_muted, text_primary, text_secondary,
    workspace::RustduckWorkspace,
};
use gpui::{div, prelude::*, px, AnyElement, Context, Entity, IntoElement};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::resizable::{h_resizable, resizable_panel};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit_assets::IconName;

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
                            this.tabs[tab_idx].show_hidden = !this.tabs[tab_idx].show_hidden;
                            cx.notify();
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
    let raw_entries = if is_local {
        &tab.local_entries
    } else {
        &tab.remote_entries
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
    let action_input = if is_local {
        &tab.local_action_input
    } else {
        &tab.remote_action_input
    };
    let is_editing_path = if is_local {
        tab.local_path_editing
    } else {
        tab.remote_path_editing
    };
    let inline_action = if is_local {
        tab.local_inline_action.clone()
    } else {
        tab.remote_inline_action.clone()
    };
    let selected_path = if is_local {
        tab.selected_local_path.clone()
    } else {
        tab.selected_remote_path.clone()
    };
    let selected_entry = ws.selected_entry(tab_idx, is_local);

    let query = filter_input.read(cx).value().to_string();
    let entries = filter_and_sort_entries(
        raw_entries,
        &query,
        tab.show_hidden,
        tab.sort_column,
        tab.sort_ascending,
    );
    let (dir_count, file_count, total_bytes) = summarize_pane_entries(&entries);
    let breadcrumbs = split_breadcrumb_segments(current_path, is_local);
    let has_parent = parent_local_or_remote(current_path, is_local).is_some();

    let action_header = render_pane_action_header(
        tab_idx,
        is_local,
        &tab.title,
        tab.form.protocol,
        filter_input,
        selected_entry.clone(),
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
    let inline_action_bar =
        inline_action.map(|act| render_pane_inline_action(tab_idx, is_local, act, action_input, cx));
    let column_header = render_column_header(
        tab_idx,
        is_local,
        tab.sort_column,
        tab.sort_ascending,
        cx,
    );
    let file_table = render_pane_file_table(
        tab_idx,
        is_local,
        has_parent,
        entries,
        selected_path,
        cx,
    );
    let footer = render_pane_footer(dir_count, file_count, total_bytes, selected_entry);

    v_flex()
        .size_full()
        .bg(bg_canvas())
        .child(action_header)
        .child(breadcrumb_bar)
        .when_some(inline_action_bar, |el, bar| el.child(bar))
        .child(column_header)
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
    selected_entry: Option<FileEntry>,
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

    let selected_for_transfer = selected_entry.clone();
    let selected_for_rename = selected_entry.clone();
    let selected_for_delete = selected_entry;

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
                .when_some(selected_for_transfer, |bar, sel_entry| {
                    if matches!(sel_entry.kind, FileKind::Bucket) {
                        bar
                    } else {
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
                                    this.enqueue_file_transfer(tab_idx, is_local, &sel_entry, cx);
                                })),
                        )
                    }
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
                .when_some(selected_for_rename, |bar, sel_entry| {
                    if matches!(sel_entry.kind, FileKind::Bucket) {
                        bar
                    } else {
                        bar.child(
                            Button::new((side_prefix, 5usize))
                                .xsmall()
                                .secondary()
                                .icon(IconName::Pencil)
                                .tooltip(t.rename_selected_tooltip())
                                .on_click(cx.listener(move |this, _ev, window, cx| {
                                    this.open_rename_prompt(
                                        tab_idx, is_local, &sel_entry, window, cx,
                                    );
                                })),
                        )
                    }
                })
                .when_some(selected_for_delete, |bar, sel_entry| {
                    bar.child(
                        Button::new((side_prefix, 6usize))
                            .xsmall()
                            .danger()
                            .icon(IconName::Trash)
                            .tooltip(t.delete_selected_tooltip())
                            .on_click(cx.listener(move |this, _ev, _win, cx| {
                                this.open_delete_confirm(tab_idx, is_local, &sel_entry, cx);
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
                    .on_click(cx.listener(move |this, _ev, _win, cx| {
                        if is_local {
                            this.tabs[tab_idx].local_path_editing = true;
                        } else {
                            this.tabs[tab_idx].remote_path_editing = true;
                        }
                        cx.notify();
                    })),
            )
        })
        .into_any_element()
}

fn render_pane_inline_action(
    tab_idx: usize,
    is_local: bool,
    action: PaneInlineAction,
    action_input: &Entity<InputState>,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let side_prefix = if is_local { "local" } else { "remote" };
    match action {
        PaneInlineAction::NewFolder => h_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_2()
            .bg(bg_row_selected())
            .border_b_1()
            .border_color(border_selected())
            .items_center()
            .child(
                Icon::new(IconName::FolderPlus)
                    .xsmall()
                    .text_color(accent_blue()),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(text_primary())
                    .child(t.inline_new_folder_label()),
            )
            .child(div().flex_1().child(Input::new(action_input).xsmall()))
            .child(
                Button::new((side_prefix, 20usize))
                    .xsmall()
                    .primary()
                    .icon(IconName::Check)
                    .label(t.create_btn())
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.submit_inline_action(tab_idx, is_local, window, cx);
                    })),
            )
            .child(
                Button::new((side_prefix, 21usize))
                    .xsmall()
                    .ghost()
                    .icon(IconName::X)
                    .label(t.cancel_btn())
                    .on_click(cx.listener(move |this, _ev, _win, cx| {
                        this.cancel_inline_action(tab_idx, is_local, cx);
                    })),
            )
            .into_any_element(),
        PaneInlineAction::Rename { original_name, .. } => h_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_2()
            .bg(bg_row_selected())
            .border_b_1()
            .border_color(border_selected())
            .items_center()
            .child(
                Icon::new(IconName::Pencil)
                    .xsmall()
                    .text_color(accent_amber()),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(text_primary())
                    .child(t.inline_rename_label(&original_name)),
            )
            .child(div().flex_1().child(Input::new(action_input).xsmall()))
            .child(
                Button::new((side_prefix, 22usize))
                    .xsmall()
                    .primary()
                    .icon(IconName::Check)
                    .label(t.confirm_btn())
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.submit_inline_action(tab_idx, is_local, window, cx);
                    })),
            )
            .child(
                Button::new((side_prefix, 23usize))
                    .xsmall()
                    .ghost()
                    .icon(IconName::X)
                    .label(t.cancel_btn())
                    .on_click(cx.listener(move |this, _ev, _win, cx| {
                        this.cancel_inline_action(tab_idx, is_local, cx);
                    })),
            )
            .into_any_element(),
        PaneInlineAction::ConfirmDelete { entry } => h_flex()
            .w_full()
            .px_3()
            .py_2()
            .gap_2()
            .bg(bg_elevated())
            .border_b_1()
            .border_color(accent_red())
            .justify_between()
            .items_center()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(IconName::Trash).xsmall().text_color(accent_red()))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(text_primary())
                            .child(
                                t.inline_delete_confirm_prompt(
                                    entry.kind.is_dir_like(),
                                    &entry.name,
                                ),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .child(
                        Button::new((side_prefix, 24usize))
                            .xsmall()
                            .danger()
                            .icon(IconName::Check)
                            .label(t.confirm_delete_btn())
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                this.submit_inline_action(tab_idx, is_local, window, cx);
                            })),
                    )
                    .child(
                        Button::new((side_prefix, 25usize))
                            .xsmall()
                            .ghost()
                            .icon(IconName::X)
                            .label(t.cancel_btn())
                            .on_click(cx.listener(move |this, _ev, _win, cx| {
                                this.cancel_inline_action(tab_idx, is_local, cx);
                            })),
                    ),
            )
            .into_any_element(),
    }
}

fn render_column_header(
    tab_idx: usize,
    is_local: bool,
    active_col: SortColumn,
    ascending: bool,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let side_offset: usize = if is_local { 300 } else { 400 };
    let arrow = if ascending { "↑" } else { "↓" };
    let name_base = t.col_name();
    let mod_base = t.col_modified();
    let size_base = t.col_size();

    let name_label = if active_col == SortColumn::Name {
        format!("{name_base} {arrow}")
    } else {
        name_base.to_string()
    };
    let mod_label = if active_col == SortColumn::Modified {
        format!("{mod_base} {arrow}")
    } else {
        mod_base.to_string()
    };
    let size_label = if active_col == SortColumn::Size {
        format!("{size_base} {arrow}")
    } else {
        size_base.to_string()
    };

    h_flex()
        .w_full()
        .h(px(28.0))
        .px_3()
        .bg(bg_panel())
        .border_b_1()
        .border_color(border_subtle())
        .items_center()
        .child(
            div().flex_1().child(
                Button::new(("sort-name", side_offset))
                    .xsmall()
                    .ghost()
                    .compact()
                    .label(name_label)
                    .on_click(cx.listener(move |this, _ev, _win, cx| {
                        this.toggle_sort(tab_idx, SortColumn::Name, cx);
                    })),
            ),
        )
        .child(
            div().w(px(136.0)).child(
                Button::new(("sort-mod", side_offset + 1))
                    .xsmall()
                    .ghost()
                    .compact()
                    .label(mod_label)
                    .on_click(cx.listener(move |this, _ev, _win, cx| {
                        this.toggle_sort(tab_idx, SortColumn::Modified, cx);
                    })),
            ),
        )
        .child(
            div()
                .w(px(88.0))
                .flex()
                .justify_end()
                .child(
                    Button::new(("sort-size", side_offset + 2))
                        .xsmall()
                        .ghost()
                        .compact()
                        .label(size_label)
                        .on_click(cx.listener(move |this, _ev, _win, cx| {
                            this.toggle_sort(tab_idx, SortColumn::Size, cx);
                        })),
                ),
        )
        .child(
            div()
                .w(px(86.0))
                .flex()
                .justify_end()
                .text_xs()
                .text_color(text_muted())
                .child(t.col_actions()),
        )
        .into_any_element()
}

fn render_pane_file_table(
    tab_idx: usize,
    is_local: bool,
    has_parent: bool,
    entries: Vec<FileEntry>,
    selected_path: Option<String>,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let side_prefix = if is_local { "local" } else { "remote" };
    let rows: Vec<AnyElement> = if entries.is_empty() {
        vec![v_flex()
            .w_full()
            .py_10()
            .gap_2()
            .items_center()
            .justify_center()
            .child(
                Icon::new(IconName::FolderOpen)
                    .large()
                    .text_color(text_muted()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(text_muted())
                    .child(t.empty_directory_text()),
            )
            .into_any_element()]
    } else {
        entries
            .into_iter()
            .enumerate()
            .map(|(row_idx, entry)| {
                let is_selected = selected_path.as_deref() == Some(entry.path.as_str());
                render_entry_row(tab_idx, row_idx, entry, is_local, is_selected, cx)
            })
            .collect()
    };

    v_flex()
        .id((side_prefix, 900 + tab_idx))
        .flex_1()
        .w_full()
        .overflow_y_scrollbar()
        .when(has_parent, |list| {
            list.child(
                h_flex()
                    .id((side_prefix, 9999usize))
                    .w_full()
                    .h(px(28.0))
                    .px_3()
                    .bg(bg_row_alt())
                    .border_b_1()
                    .border_color(border_subtle())
                    .hover(|s| s.bg(bg_row_hover()))
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .on_click(cx.listener(
                        move |this, ev: &gpui::ClickEvent, window, cx| {
                            if ev.click_count() >= 2 {
                                this.navigate_up(tab_idx, is_local, window, cx);
                            }
                        },
                    ))
                    .child(
                        Icon::new(IconName::FolderOpen)
                            .xsmall()
                            .text_color(text_muted()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(text_muted())
                            .child(t.parent_dir_row_text()),
                    ),
            )
        })
        .children(rows)
        .into_any_element()
}

fn render_pane_footer(
    dir_count: usize,
    file_count: usize,
    total_bytes: u64,
    selected_entry: Option<FileEntry>,
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
                .text_color(if selected_entry.is_some() {
                    accent_blue()
                } else {
                    text_muted()
                })
                .child(t.pane_footer_selection(selected_entry.as_ref().map(|e| e.name.as_str()))),
        )
        .into_any_element()
}

fn render_entry_row(
    tab_idx: usize,
    row_idx: usize,
    entry: FileEntry,
    is_local: bool,
    is_selected: bool,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let (icon_name, icon_color) = entry_icon_and_color(&entry);
    let size_text = if entry.kind.is_dir_like() {
        "--".to_string()
    } else {
        format_bytes(entry.size)
    };
    let modified_text = format_modified_time(entry.modified_unix);
    let is_dir = entry.kind.is_dir_like();
    let is_bucket = matches!(entry.kind, FileKind::Bucket);

    let entry_click = entry.clone();
    let entry_action_primary = entry.clone();
    let entry_rename = entry.clone();
    let entry_del = entry.clone();
    let side_tag: usize = if is_local { 10000 } else { 20000 };

    let row_bg = if is_selected {
        bg_row_selected()
    } else if row_idx % 2 == 1 {
        bg_row_alt()
    } else {
        bg_canvas()
    };

    h_flex()
        .id(("file-table-row", side_tag + row_idx))
        .w_full()
        .h(px(30.0))
        .px_3()
        .bg(row_bg)
        .border_b_1()
        .border_color(if is_selected {
            border_selected()
        } else {
            border_subtle()
        })
        .hover(|s| {
            if is_selected {
                s.bg(bg_row_selected())
            } else {
                s.bg(bg_row_hover())
            }
        })
        .items_center()
        .cursor_pointer()
        .on_click(cx.listener(
            move |this, ev: &gpui::ClickEvent, window, cx| {
                this.select_entry(tab_idx, is_local, Some(entry_click.path.clone()), cx);
                if ev.click_count() >= 2 {
                    if entry_click.kind.is_dir_like() {
                        this.enter_directory(tab_idx, is_local, &entry_click, window, cx);
                    } else {
                        this.enqueue_file_transfer(tab_idx, is_local, &entry_click, cx);
                    }
                }
            },
        ))
        .child(
            h_flex()
                .flex_1()
                .gap_2()
                .items_center()
                .overflow_hidden()
                .child(Icon::new(icon_name).xsmall().text_color(icon_color))
                .child(
                    div()
                        .text_xs()
                        .font_weight(if is_dir {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(if is_selected {
                            accent_blue()
                        } else {
                            text_primary()
                        })
                        .child(entry.name.clone()),
                ),
        )
        .child(
            div()
                .w(px(136.0))
                .text_xs()
                .text_color(text_muted())
                .child(modified_text),
        )
        .child(
            div()
                .w(px(88.0))
                .flex()
                .justify_end()
                .text_xs()
                .text_color(text_secondary())
                .child(size_text),
        )
        .child(
            h_flex()
                .w(px(86.0))
                .gap_1()
                .justify_end()
                .items_center()
                .when(!is_bucket, |row| {
                    row.child(
                        Button::new(("row-transfer", side_tag + row_idx))
                            .xsmall()
                            .ghost()
                            .compact()
                            .icon(if is_local {
                                IconName::Upload
                            } else {
                                IconName::Download
                            })
                            .tooltip(if is_local {
                                t.upload_selected_tooltip()
                            } else {
                                t.download_selected_tooltip()
                            })
                            .on_click(cx.listener(move |this, _ev, _win, cx| {
                                this.enqueue_file_transfer(
                                    tab_idx,
                                    is_local,
                                    &entry_action_primary,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new(("row-rename", side_tag + row_idx))
                            .xsmall()
                            .ghost()
                            .compact()
                            .icon(IconName::Pencil)
                            .tooltip(t.rename_selected_tooltip())
                            .on_click(cx.listener(move |this, _ev, window, cx| {
                                this.open_rename_prompt(
                                    tab_idx,
                                    is_local,
                                    &entry_rename,
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        Button::new(("row-del", side_tag + row_idx))
                            .xsmall()
                            .ghost()
                            .compact()
                            .icon(IconName::Trash)
                            .tooltip(t.delete_selected_tooltip())
                            .on_click(cx.listener(move |this, _ev, _win, cx| {
                                this.open_delete_confirm(tab_idx, is_local, &entry_del, cx);
                            })),
                    )
                }),
        )
        .into_any_element()
}
