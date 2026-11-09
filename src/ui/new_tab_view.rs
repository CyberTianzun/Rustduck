use crate::i18n::i18n;
use crate::models::{Bookmark, ProtocolKind, S3AddressingStyle, S3Preset};
use crate::ui::{
    accent_amber, accent_blue, bg_canvas, bg_elevated, bg_panel, bg_row_hover, bg_row_selected,
    border_selected, border_strong, border_subtle, protocol_icon_and_color, text_muted,
    text_primary, text_secondary,
    workspace::{ConnectionFormInputs, RustduckWorkspace},
};
use gpui::{div, prelude::*, px, AnyElement, Context, IntoElement};
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit_assets::IconName;
use uuid::Uuid;

pub fn render_new_tab_view(
    tab_idx: usize,
    ws: &RustduckWorkspace,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let left_gallery = render_favorites_gallery(tab_idx, ws, cx);
    let right_inspector = render_quick_connect_inspector(tab_idx, ws, cx);

    h_flex()
        .size_full()
        .bg(bg_canvas())
        .child(left_gallery)
        .child(right_inspector)
        .into_any_element()
}

fn render_favorites_gallery(
    tab_idx: usize,
    ws: &RustduckWorkspace,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let tab = &ws.tabs[tab_idx];
    let form = &tab.form;
    let selected_bookmark_id = form.bookmark_id;
    let search_query = form
        .bookmark_search_input
        .read(cx)
        .value()
        .trim()
        .to_lowercase();

    let filtered_bookmarks: Vec<Bookmark> = ws
        .bookmark_store
        .list()
        .iter()
        .filter(|bm| {
            if search_query.is_empty() {
                true
            } else {
                bm.name.to_lowercase().contains(&search_query)
                    || bm.config.summary().to_lowercase().contains(&search_query)
            }
        })
        .cloned()
        .collect();

    let header = render_favorites_header(
        tab_idx,
        filtered_bookmarks.len(),
        &form.bookmark_search_input,
        cx,
    );
    let list_body = render_favorites_list(tab_idx, filtered_bookmarks, selected_bookmark_id, cx);
    let footer = h_flex()
        .w_full()
        .h(px(28.0))
        .px_4()
        .bg(bg_panel())
        .border_t_1()
        .border_color(border_subtle())
        .items_center()
        .child(
            div()
                .text_xs()
                .text_color(text_muted())
                .child(t.favorites_footer_tip()),
        )
        .into_any_element();

    v_flex()
        .w(gpui::relative(0.44))
        .h_full()
        .bg(bg_canvas())
        .border_r_1()
        .border_color(border_subtle())
        .child(header)
        .child(list_body)
        .child(footer)
        .into_any_element()
}

fn render_favorites_header(
    tab_idx: usize,
    count: usize,
    search_input: &gpui::Entity<InputState>,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    v_flex()
        .w_full()
        .px_4()
        .py_3()
        .gap_2p5()
        .bg(bg_panel())
        .border_b_1()
        .border_color(border_subtle())
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Icon::new(IconName::Star).small().text_color(accent_amber()))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(text_primary())
                                .child(t.favorites_title()),
                        )
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded(px(99.0))
                                .bg(bg_elevated())
                                .text_xs()
                                .text_color(text_muted())
                                .child(format!("{count}")),
                        ),
                )
                .child(
                    Button::new("reset-quick-connect")
                        .xsmall()
                        .secondary()
                        .icon(IconName::Plus)
                        .label(t.new_blank_conn_btn())
                        .on_click(cx.listener(move |this, _ev, window, cx| {
                            this.reset_form_for_new_connection(tab_idx, window, cx);
                        })),
                ),
        )
        .child(
            Input::new(search_input)
                .small()
                .prefix(Icon::new(IconName::Search).xsmall().text_color(text_muted()))
                .cleanable(true),
        )
        .into_any_element()
}

fn render_favorites_list(
    tab_idx: usize,
    filtered_bookmarks: Vec<Bookmark>,
    selected_bookmark_id: Option<Uuid>,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let children: Vec<AnyElement> = if filtered_bookmarks.is_empty() {
        vec![v_flex()
            .w_full()
            .py_12()
            .px_6()
            .gap_3()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(48.0))
                    .rounded(px(12.0))
                    .bg(bg_panel())
                    .border_1()
                    .border_color(border_subtle())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(IconName::Server)
                            .large()
                            .text_color(text_muted()),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(text_secondary())
                    .child(t.favorites_empty_title()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(text_muted())
                    .child(t.favorites_empty_desc()),
            )
            .into_any_element()]
    } else {
        filtered_bookmarks
            .into_iter()
            .enumerate()
            .map(|(bm_idx, bm)| {
                let is_selected = selected_bookmark_id == Some(bm.id);
                render_bookmark_card(tab_idx, bm_idx, bm, is_selected, cx)
            })
            .collect()
    };

    v_flex()
        .id(("favorites-gallery-scroll", tab_idx))
        .flex_1()
        .w_full()
        .p_3()
        .gap_2()
        .overflow_y_scrollbar()
        .children(children)
        .into_any_element()
}

fn render_bookmark_card(
    tab_idx: usize,
    bm_idx: usize,
    bm: Bookmark,
    is_selected: bool,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let bm_id = bm.id;
    let bm_select = bm.clone();
    let bm_connect = bm.clone();
    let proto = bm.config.kind();
    let (proto_icon, proto_color) = protocol_icon_and_color(proto);
    let proto_tag = match proto {
        ProtocolKind::Sftp => "SFTP",
        ProtocolKind::S3 => "S3",
        ProtocolKind::Smb => "SMB",
    };

    h_flex()
        .id(("fav-server-card", bm_idx))
        .w_full()
        .p_3()
        .rounded(px(8.0))
        .bg(if is_selected {
            bg_row_selected()
        } else {
            bg_panel()
        })
        .border_1()
        .border_color(if is_selected {
            border_selected()
        } else {
            border_subtle()
        })
        .hover(|s| {
            if is_selected {
                s.bg(bg_row_selected())
            } else {
                s.bg(bg_row_hover()).border_color(border_strong())
            }
        })
        .justify_between()
        .items_center()
        .cursor_pointer()
        .on_click(cx.listener(
            move |this, ev: &gpui::ClickEvent, window, cx| {
                this.load_bookmark_into_form(tab_idx, &bm_select, window, cx);
                if ev.click_count() >= 2 {
                    this.connect_tab(tab_idx, cx);
                }
            },
        ))
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .overflow_hidden()
                .child(
                    div()
                        .size(px(36.0))
                        .rounded(px(8.0))
                        .bg(bg_elevated())
                        .border_1()
                        .border_color(border_subtle())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(proto_icon).small().text_color(proto_color)),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .overflow_hidden()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(text_primary())
                                        .child(bm.name.clone()),
                                )
                                .child(
                                    div()
                                        .px_1p5()
                                        .py_0()
                                        .rounded(px(4.0))
                                        .bg(bg_elevated())
                                        .text_xs()
                                        .text_color(proto_color)
                                        .child(proto_tag),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(text_muted())
                                .child(bm.config.summary()),
                        ),
                ),
        )
        .child(
            h_flex()
                .gap_1p5()
                .items_center()
                .child(
                    Button::new(("conn-bm", bm_idx))
                        .xsmall()
                        .primary()
                        .icon(IconName::Play)
                        .label(t.connect_short_btn())
                        .on_click(cx.listener(move |this, _ev, window, cx| {
                            this.load_bookmark_into_form(tab_idx, &bm_connect, window, cx);
                            this.connect_tab(tab_idx, cx);
                        })),
                )
                .child(
                    Button::new(("del-bm", bm_idx))
                        .xsmall()
                        .ghost()
                        .icon(IconName::Trash)
                        .tooltip(t.remove_favorite_tooltip())
                        .on_click(cx.listener(move |this, _ev, _win, cx| {
                            let _ = this.bookmark_store.remove(bm_id);
                            if this.tabs[tab_idx].form.bookmark_id == Some(bm_id) {
                                this.tabs[tab_idx].form.bookmark_id = None;
                            }
                            cx.notify();
                        })),
                ),
        )
        .into_any_element()
}

fn render_quick_connect_inspector(
    tab_idx: usize,
    ws: &RustduckWorkspace,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let tab = &ws.tabs[tab_idx];
    let form = &tab.form;
    let active_protocol = form.protocol;
    let has_bookmark = form.bookmark_id.is_some();
    let is_connecting = tab.is_connecting;

    let header = render_inspector_header(tab_idx, has_bookmark, active_protocol, cx);
    let body = render_inspector_body(tab_idx, form, cx);
    let footer = render_inspector_footer(tab_idx, is_connecting, cx);

    v_flex()
        .w(gpui::relative(0.56))
        .h_full()
        .bg(bg_panel())
        .child(header)
        .child(body)
        .child(footer)
        .into_any_element()
}

fn render_inspector_header(
    tab_idx: usize,
    has_bookmark: bool,
    active_protocol: ProtocolKind,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    v_flex()
        .w_full()
        .px_6()
        .py_3()
        .gap_3()
        .border_b_1()
        .border_color(border_subtle())
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Icon::new(IconName::Network).small().text_color(accent_blue()))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(text_primary())
                                .child(t.inspector_title(has_bookmark)),
                        ),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(Icon::new(IconName::Lock).xsmall().text_color(text_muted()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(text_muted())
                                .child(t.keyring_badge_text()),
                        ),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .p_1()
                .gap_1()
                .rounded(px(8.0))
                .bg(bg_canvas())
                .border_1()
                .border_color(border_subtle())
                .child(
                    div().flex_1().child(
                        Button::new("proto-sftp")
                            .small()
                            .w_full()
                            .icon(IconName::Terminal)
                            .with_variant(if active_protocol == ProtocolKind::Sftp {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .label(t.proto_sftp_label())
                            .on_click(cx.listener(move |this, _ev, _win, cx| {
                                this.tabs[tab_idx].form.protocol = ProtocolKind::Sftp;
                                cx.notify();
                            })),
                    ),
                )
                .child(
                    div().flex_1().child(
                        Button::new("proto-s3")
                            .small()
                            .w_full()
                            .icon(IconName::Cloud)
                            .with_variant(if active_protocol == ProtocolKind::S3 {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .label(t.proto_s3_label())
                            .on_click(cx.listener(move |this, _ev, _win, cx| {
                                this.tabs[tab_idx].form.protocol = ProtocolKind::S3;
                                cx.notify();
                            })),
                    ),
                )
                .child(
                    div().flex_1().child(
                        Button::new("proto-smb")
                            .small()
                            .w_full()
                            .icon(IconName::HardDrive)
                            .with_variant(if active_protocol == ProtocolKind::Smb {
                                ButtonVariant::Primary
                            } else {
                                ButtonVariant::Ghost
                            })
                            .label(t.proto_smb_label())
                            .on_click(cx.listener(move |this, _ev, _win, cx| {
                                this.tabs[tab_idx].form.protocol = ProtocolKind::Smb;
                                cx.notify();
                            })),
                    ),
                ),
        )
        .into_any_element()
}

fn render_inspector_body(
    tab_idx: usize,
    form: &ConnectionFormInputs,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let alias_card = v_flex()
        .w_full()
        .p_3p5()
        .gap_2p5()
        .rounded(px(8.0))
        .bg(bg_canvas())
        .border_1()
        .border_color(border_subtle())
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(text_secondary())
                        .child(t.label_display_name()),
                )
                .child(
                    Input::new(&form.name_input)
                        .small()
                        .prefix(Icon::new(IconName::Star).xsmall().text_color(text_muted()))
                        .cleanable(true),
                ),
        )
        .into_any_element();

    let protocol_card = match form.protocol {
        ProtocolKind::Sftp => render_sftp_form_card(form),
        ProtocolKind::S3 => render_s3_form_card(tab_idx, form, cx),
        ProtocolKind::Smb => render_smb_form_card(form),
    };

    v_flex()
        .id(("inspector-form-scroll", tab_idx))
        .flex_1()
        .w_full()
        .px_6()
        .py_4()
        .gap_4()
        .overflow_y_scrollbar()
        .child(alias_card)
        .child(protocol_card)
        .into_any_element()
}

fn render_sftp_form_card(form: &ConnectionFormInputs) -> AnyElement {
    let t = i18n();
    let row_host_port = h_flex()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_sftp_host()))
                .child(
                    Input::new(&form.sftp_host_input)
                        .small()
                        .prefix(Icon::new(IconName::Server).xsmall().text_color(text_muted()))
                        .cleanable(true),
                ),
        )
        .child(
            v_flex()
                .w(px(100.0))
                .gap_1()
                .child(field_label(t.label_port()))
                .child(Input::new(&form.sftp_port_input).small()),
        )
        .into_any_element();

    let row_user_secret = h_flex()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_username()))
                .child(Input::new(&form.sftp_user_input).small().cleanable(true)),
        )
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_sftp_secret()))
                .child(Input::new(&form.secret_input).small().mask_toggle()),
        )
        .into_any_element();

    let row_key_path = h_flex()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_sftp_key_path()))
                .child(
                    Input::new(&form.sftp_key_path_input)
                        .small()
                        .prefix(Icon::new(IconName::Key).xsmall().text_color(text_muted()))
                        .cleanable(true),
                ),
        )
        .child(
            v_flex()
                .w(px(180.0))
                .gap_1()
                .child(field_label(t.label_initial_remote_dir()))
                .child(
                    Input::new(&form.initial_path_input)
                        .small()
                        .prefix(
                            Icon::new(IconName::FolderOpen)
                                .xsmall()
                                .text_color(text_muted()),
                        ),
                ),
        )
        .into_any_element();

    v_flex()
        .w_full()
        .p_3p5()
        .gap_3()
        .rounded(px(8.0))
        .bg(bg_canvas())
        .border_1()
        .border_color(border_subtle())
        .child(row_host_port)
        .child(row_user_secret)
        .child(row_key_path)
        .into_any_element()
}

fn render_s3_form_card(
    tab_idx: usize,
    form: &ConnectionFormInputs,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let preset_bar = render_s3_preset_bar(tab_idx, form.s3_preset, cx);
    let row_endpoint = h_flex()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_s3_endpoint()))
                .child(
                    Input::new(&form.s3_endpoint_input)
                        .small()
                        .prefix(Icon::new(IconName::Globe).xsmall().text_color(text_muted()))
                        .cleanable(true),
                ),
        )
        .child(
            v_flex()
                .w(px(150.0))
                .gap_1()
                .child(field_label(t.label_s3_region()))
                .child(Input::new(&form.s3_region_input).small().cleanable(true)),
        )
        .into_any_element();

    let row_addressing = render_s3_addressing_row(tab_idx, form.s3_addressing, cx);

    let row_keys = h_flex()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label("Access Key ID / SecretId"))
                .child(
                    Input::new(&form.s3_access_key_input)
                        .small()
                        .prefix(Icon::new(IconName::Key).xsmall().text_color(text_muted()))
                        .cleanable(true),
                ),
        )
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label("Secret Access Key / SecretKey"))
                .child(Input::new(&form.secret_input).small().mask_toggle()),
        )
        .into_any_element();

    let row_bucket = h_flex()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_s3_bucket()))
                .child(
                    Input::new(&form.s3_bucket_input)
                        .small()
                        .prefix(
                            Icon::new(IconName::Database)
                                .xsmall()
                                .text_color(text_muted()),
                        )
                        .cleanable(true),
                ),
        )
        .child(
            v_flex()
                .w(px(180.0))
                .gap_1()
                .child(field_label(t.label_s3_prefix()))
                .child(
                    Input::new(&form.initial_path_input)
                        .small()
                        .prefix(
                            Icon::new(IconName::FolderOpen)
                                .xsmall()
                                .text_color(text_muted()),
                        ),
                ),
        )
        .into_any_element();

    v_flex()
        .w_full()
        .p_3p5()
        .gap_3()
        .rounded(px(8.0))
        .bg(bg_canvas())
        .border_1()
        .border_color(border_subtle())
        .child(preset_bar)
        .child(row_endpoint)
        .child(row_addressing)
        .child(row_keys)
        .child(row_bucket)
        .into_any_element()
}

fn render_s3_preset_bar(
    tab_idx: usize,
    s3_preset: S3Preset,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let buttons: Vec<AnyElement> = [
        S3Preset::AlibabaOss,
        S3Preset::TencentCos,
        S3Preset::Minio,
        S3Preset::AwsS3,
        S3Preset::Custom,
    ]
    .into_iter()
    .enumerate()
    .map(|(idx, preset)| {
        let active = s3_preset == preset;
        let mut btn = Button::new(("s3-preset", idx))
            .xsmall()
            .with_variant(if active {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            })
            .label(t.s3_preset_label(preset));
        if active {
            btn = btn.icon(IconName::Check);
        }
        btn.on_click(cx.listener(move |this, _ev, window, cx| {
            this.apply_s3_preset(tab_idx, preset, window, cx);
        }))
        .into_any_element()
    })
    .collect();

    v_flex()
        .gap_1p5()
        .child(field_label(t.label_s3_presets()))
        .child(h_flex().gap_1p5().flex_wrap().children(buttons))
        .into_any_element()
}

fn render_s3_addressing_row(
    tab_idx: usize,
    s3_style: S3AddressingStyle,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    h_flex()
        .w_full()
        .justify_between()
        .items_center()
        .child(field_label(t.label_s3_addressing_mode()))
        .child(
            h_flex()
                .gap_1p5()
                .child(
                    Button::new("s3-style-vh")
                        .xsmall()
                        .with_variant(if s3_style == S3AddressingStyle::VirtualHosted {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Secondary
                        })
                        .label(t.s3_addressing_label(S3AddressingStyle::VirtualHosted))
                        .on_click(cx.listener(move |this, _ev, _win, cx| {
                            this.tabs[tab_idx].form.s3_addressing =
                                S3AddressingStyle::VirtualHosted;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("s3-style-path")
                        .xsmall()
                        .with_variant(if s3_style == S3AddressingStyle::PathStyle {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Secondary
                        })
                        .label(t.s3_addressing_label(S3AddressingStyle::PathStyle))
                        .on_click(cx.listener(move |this, _ev, _win, cx| {
                            this.tabs[tab_idx].form.s3_addressing = S3AddressingStyle::PathStyle;
                            cx.notify();
                        })),
                ),
        )
        .into_any_element()
}

fn render_smb_form_card(form: &ConnectionFormInputs) -> AnyElement {
    let t = i18n();
    let row_server = h_flex()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_smb_server()))
                .child(
                    Input::new(&form.smb_server_input)
                        .small()
                        .prefix(Icon::new(IconName::Server).xsmall().text_color(text_muted()))
                        .cleanable(true),
                ),
        )
        .child(
            v_flex()
                .w(px(95.0))
                .gap_1()
                .child(field_label(t.label_port()))
                .child(Input::new(&form.smb_port_input).small()),
        )
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_smb_share()))
                .child(
                    Input::new(&form.smb_share_input)
                        .small()
                        .prefix(
                            Icon::new(IconName::HardDrive)
                                .xsmall()
                                .text_color(text_muted()),
                        )
                        .cleanable(true),
                ),
        )
        .into_any_element();

    let row_auth = h_flex()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_username()))
                .child(Input::new(&form.smb_user_input).small().cleanable(true)),
        )
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_password()))
                .child(Input::new(&form.secret_input).small().mask_toggle()),
        )
        .into_any_element();

    let row_domain = h_flex()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(field_label(t.label_smb_domain()))
                .child(Input::new(&form.smb_domain_input).small().cleanable(true)),
        )
        .child(
            v_flex()
                .w(px(180.0))
                .gap_1()
                .child(field_label(t.label_initial_remote_dir()))
                .child(
                    Input::new(&form.initial_path_input)
                        .small()
                        .prefix(
                            Icon::new(IconName::FolderOpen)
                                .xsmall()
                                .text_color(text_muted()),
                        ),
                ),
        )
        .into_any_element();

    v_flex()
        .w_full()
        .p_3p5()
        .gap_3()
        .rounded(px(8.0))
        .bg(bg_canvas())
        .border_1()
        .border_color(border_subtle())
        .child(row_server)
        .child(row_auth)
        .child(row_domain)
        .into_any_element()
}

fn render_inspector_footer(
    tab_idx: usize,
    is_connecting: bool,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    h_flex()
        .w_full()
        .h(px(54.0))
        .px_6()
        .bg(bg_panel())
        .border_t_1()
        .border_color(border_subtle())
        .justify_between()
        .items_center()
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("test-conn-btn")
                        .small()
                        .secondary()
                        .icon(IconName::CircleCheck)
                        .loading(is_connecting)
                        .label(t.test_conn_btn())
                        .on_click(cx.listener(move |this, _ev, _win, cx| {
                            this.test_connection_tab(tab_idx, cx);
                        })),
                )
                .child(
                    Button::new("save-bookmark-btn")
                        .small()
                        .secondary()
                        .icon(IconName::Star)
                        .label(t.save_bookmark_btn())
                        .on_click(cx.listener(move |this, _ev, _win, cx| {
                            this.save_current_form_as_bookmark(tab_idx, cx);
                        })),
                ),
        )
        .child(
            Button::new("connect-now-btn")
                .small()
                .primary()
                .icon(IconName::Play)
                .loading(is_connecting)
                .label(t.connect_now_btn(is_connecting))
                .on_click(cx.listener(move |this, _ev, _win, cx| {
                    this.connect_tab(tab_idx, cx);
                })),
        )
        .into_any_element()
}

fn field_label(text: &'static str) -> AnyElement {
    div()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(text_secondary())
        .child(text)
        .into_any_element()
}
