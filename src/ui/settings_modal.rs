use crate::i18n::i18n;
use crate::models::ProtocolKind;
use crate::services::preferences::{
    AppLanguage, LocalStartupDirMode, S3UploadAcl, ThemePreference,
};
use crate::services::update_checker::UpdateCheckState;
use crate::ui::workspace::RustduckWorkspace;
use crate::ui::{
    accent_amber, accent_blue, accent_green, accent_red, bg_canvas, bg_elevated, bg_panel,
    bg_row_hover, bg_row_selected, border_selected, border_strong, border_subtle,
    protocol_icon_and_color, text_muted, text_primary, text_secondary,
};
use gpui::{
    div, img, prelude::*, px, AnyElement, Context, FontWeight, IntoElement, MouseButton,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, Disableable as _, Icon, Sizable as _};
use gpui_kit_assets::IconName;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SETTINGS_MODAL_WIDTH_PX: f32 = 820.0;
pub const SETTINGS_MODAL_HEIGHT_PX: f32 = 620.0;
pub const SETTINGS_SIDEBAR_WIDTH_PX: f32 = 168.0;
pub const SETTINGS_CONTENT_WIDTH_PX: f32 =
    SETTINGS_MODAL_WIDTH_PX - SETTINGS_SIDEBAR_WIDTH_PX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    General,
    About,
}

pub fn render_settings_modal(
    ws: &RustduckWorkspace,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let active_tab = ws.settings_tab;
    let current_theme = ws.preferences_store.preferences().theme;
    let current_lang = ws.preferences_store.preferences().language;

    let header = h_flex()
        .w_full()
        .h(px(44.0))
        .px_4()
        .bg(bg_panel())
        .border_b_1()
        .border_color(border_subtle())
        .justify_between()
        .items_center()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(IconName::Settings).small().text_color(accent_blue()))
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(text_primary())
                        .child(t.settings_modal_title()),
                ),
        )
        .child(
            Button::new("close-settings-modal")
                .xsmall()
                .ghost()
                .compact()
                .icon(IconName::X)
                .tooltip(t.cancel_btn())
                .on_click(cx.listener(|this, _ev, _win, cx| {
                    this.settings_open = false;
                    cx.notify();
                })),
        );

    let sidebar = v_flex()
        .w(px(SETTINGS_SIDEBAR_WIDTH_PX))
        .flex_shrink_0()
        .h_full()
        .p_2p5()
        .gap_1()
        .bg(bg_panel())
        .border_r_1()
        .border_color(border_subtle())
        .child(render_sidebar_nav_item(
            "settings-nav-general",
            IconName::SlidersHorizontal,
            t.settings_tab_general(),
            active_tab == SettingsTab::General,
            SettingsTab::General,
            cx,
        ))
        .child(render_sidebar_nav_item(
            "settings-nav-about",
            IconName::Info,
            t.settings_tab_about(),
            active_tab == SettingsTab::About,
            SettingsTab::About,
            cx,
        ));

    let content_body = match active_tab {
        SettingsTab::General => render_general_settings(ws, current_theme, current_lang, cx),
        SettingsTab::About => render_about_settings(ws, cx),
    };

    let modal_card = v_flex()
        .id("settings-modal-card")
        .w(px(SETTINGS_MODAL_WIDTH_PX))
        .h(px(SETTINGS_MODAL_HEIGHT_PX))
        .rounded(px(12.0))
        .bg(bg_elevated())
        .border_1()
        .border_color(border_strong())
        .overflow_hidden()
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_ev, _win, cx| cx.stop_propagation())
        .child(header)
        .child(
            h_flex()
                .flex_1()
                .w_full()
                .min_h_0()
                .overflow_hidden()
                .child(sidebar)
                .child(
                    v_flex()
                        .id("settings-modal-content-pane")
                        .w(px(SETTINGS_CONTENT_WIDTH_PX))
                        .min_w_0()
                        .min_h_0()
                        .h_full()
                        .px_6()
                        .py_5()
                        .bg(bg_canvas())
                        .overflow_y_scrollbar()
                        .child(content_body),
                ),
        );

    div()
        .id("settings-modal-backdrop")
        .absolute()
        .inset_0()
        .bg(gpui::hsla(0.0, 0.0, 0.0, 0.52))
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _ev, _win, cx| {
            this.settings_open = false;
            cx.notify();
        }))
        .child(modal_card)
        .into_any_element()
}

fn render_sidebar_nav_item(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    selected: bool,
    target_tab: SettingsTab,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let icon_color = if selected {
        accent_blue()
    } else {
        text_muted()
    };
    let text_col = if selected {
        text_primary()
    } else {
        text_secondary()
    };

    h_flex()
        .id(id)
        .w_full()
        .px_3()
        .py_2()
        .gap_2()
        .rounded(px(8.0))
        .cursor_pointer()
        .items_center()
        .when(selected, |el| {
            el.bg(bg_row_selected())
                .border_1()
                .border_color(border_selected())
        })
        .when(!selected, |el| {
            el.border_1()
                .border_color(gpui::transparent_black())
                .hover(|s| s.bg(bg_row_hover()))
        })
        .on_click(cx.listener(move |this, _ev, _win, cx| {
            this.settings_tab = target_tab;
            cx.notify();
        }))
        .child(Icon::new(icon).small().text_color(icon_color))
        .child(
            div()
                .text_xs()
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(text_col)
                .child(label),
        )
        .into_any_element()
}

fn render_general_settings(
    ws: &RustduckWorkspace,
    current_theme: ThemePreference,
    current_lang: AppLanguage,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let prefs = ws.preferences_store.preferences();
    let current_local_mode = prefs.local_startup_mode;
    let current_s3_acl = prefs.s3_default_acl;

    let theme_section = v_flex()
        .w_full()
        .gap_2p5()
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(text_primary())
                        .child(t.settings_section_theme()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_muted())
                        .child(t.settings_section_theme_desc()),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .gap_2p5()
                .child(render_theme_card(
                    "theme-opt-system",
                    ThemePreference::System,
                    IconName::SlidersHorizontal,
                    t.theme_option_title(ThemePreference::System),
                    current_theme == ThemePreference::System,
                    cx,
                ))
                .child(render_theme_card(
                    "theme-opt-light",
                    ThemePreference::Light,
                    IconName::Sun,
                    t.theme_option_title(ThemePreference::Light),
                    current_theme == ThemePreference::Light,
                    cx,
                ))
                .child(render_theme_card(
                    "theme-opt-dark",
                    ThemePreference::Dark,
                    IconName::Moon,
                    t.theme_option_title(ThemePreference::Dark),
                    current_theme == ThemePreference::Dark,
                    cx,
                )),
        );

    let language_section = v_flex()
        .w_full()
        .gap_2p5()
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(text_primary())
                        .child(t.settings_section_language()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_muted())
                        .child(t.settings_section_language_desc()),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .gap_2p5()
                .child(render_language_card(
                    "lang-opt-zh",
                    AppLanguage::ZhCn,
                    "zh-CN",
                    current_lang == AppLanguage::ZhCn,
                    cx,
                ))
                .child(render_language_card(
                    "lang-opt-en",
                    AppLanguage::EnUs,
                    "en-US",
                    current_lang == AppLanguage::EnUs,
                    cx,
                )),
        );

    let local_dir_section = v_flex()
        .w_full()
        .gap_2p5()
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(text_primary())
                        .child(t.settings_section_local_dir()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_muted())
                        .child(t.settings_section_local_dir_desc()),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .gap_2p5()
                .child(render_local_mode_card(
                    "local-dir-mode-remember",
                    LocalStartupDirMode::RememberLast,
                    IconName::Laptop,
                    t.local_dir_mode_title(LocalStartupDirMode::RememberLast),
                    current_local_mode == LocalStartupDirMode::RememberLast,
                    cx,
                ))
                .child(render_local_mode_card(
                    "local-dir-mode-custom",
                    LocalStartupDirMode::Custom,
                    IconName::FolderOpen,
                    t.local_dir_mode_title(LocalStartupDirMode::Custom),
                    current_local_mode == LocalStartupDirMode::Custom,
                    cx,
                )),
        )
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .items_center()
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&ws.settings_custom_local_dir_input)
                            .small()
                            .prefix(
                                Icon::new(IconName::Folder)
                                    .xsmall()
                                    .text_color(text_muted()),
                            )
                            .cleanable(true),
                    ),
                )
                .child(
                    Button::new("settings-browse-local-dir")
                        .small()
                        .secondary()
                        .icon(IconName::FolderOpen)
                        .label(t.browse_folder_btn())
                        .on_click(cx.listener(|this, _ev, window, cx| {
                            this.browse_custom_local_dir_in_settings(window, cx);
                        })),
                ),
        );

    let acl_pills: Vec<AnyElement> = S3UploadAcl::ALL
        .into_iter()
        .enumerate()
        .map(|(idx, acl)| render_s3_acl_pill(idx, acl, current_s3_acl == acl, cx))
        .collect();

    let s3_acl_section = v_flex()
        .w_full()
        .min_w_0()
        .gap_2p5()
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(text_primary())
                        .child(t.settings_section_s3_acl()),
                )
                .child(
                    div()
                        .w_full()
                        .text_xs()
                        .text_color(text_muted())
                        .child(t.settings_section_s3_acl_desc()),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .flex_wrap()
                .children(acl_pills),
        );

    v_flex()
        .w_full()
        .min_w_0()
        .gap_5()
        .child(theme_section)
        .child(language_section)
        .child(local_dir_section)
        .child(s3_acl_section)
        .into_any_element()
}

fn render_local_mode_card(
    id: &'static str,
    mode: LocalStartupDirMode,
    icon: IconName,
    title: &'static str,
    selected: bool,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let icon_color = if selected {
        accent_blue()
    } else {
        text_secondary()
    };

    h_flex()
        .id(id)
        .flex_1()
        .px_3p5()
        .py_2p5()
        .gap_2()
        .rounded(px(8.0))
        .cursor_pointer()
        .items_center()
        .when(selected, |el| {
            el.bg(bg_row_selected())
                .border_1()
                .border_color(border_selected())
        })
        .when(!selected, |el| {
            el.bg(bg_elevated())
                .border_1()
                .border_color(border_subtle())
                .hover(|s| s.bg(bg_row_hover()))
        })
        .on_click(cx.listener(move |this, _ev, _window, cx| {
            this.set_local_startup_mode(mode, cx);
        }))
        .child(Icon::new(icon).small().text_color(icon_color))
        .child(
            div()
                .text_xs()
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(text_primary())
                .child(title),
        )
        .into_any_element()
}

fn render_s3_acl_pill(
    idx: usize,
    acl: S3UploadAcl,
    selected: bool,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let label = t.s3_acl_option_title(acl);
    let icon_col = if selected {
        accent_blue()
    } else {
        text_muted()
    };

    h_flex()
        .id(("s3-acl-opt", idx))
        .px_3()
        .py_1p5()
        .gap_1p5()
        .rounded(px(8.0))
        .cursor_pointer()
        .items_center()
        .when(selected, |el| {
            el.bg(bg_row_selected())
                .border_1()
                .border_color(border_selected())
        })
        .when(!selected, |el| {
            el.bg(bg_elevated())
                .border_1()
                .border_color(border_subtle())
                .hover(|s| s.bg(bg_row_hover()))
        })
        .on_click(cx.listener(move |this, _ev, _window, cx| {
            this.set_s3_default_acl(acl, cx);
        }))
        .child(Icon::new(IconName::Cloud).xsmall().text_color(icon_col))
        .child(
            div()
                .text_xs()
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if selected {
                    text_primary()
                } else {
                    text_secondary()
                })
                .child(label),
        )
        .into_any_element()
}

fn render_theme_card(
    id: &'static str,
    pref: ThemePreference,
    icon: IconName,
    title: &'static str,
    selected: bool,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let icon_color = if selected {
        accent_blue()
    } else {
        text_secondary()
    };

    v_flex()
        .id(id)
        .flex_1()
        .px_3()
        .py_3()
        .gap_2()
        .rounded(px(8.0))
        .cursor_pointer()
        .items_center()
        .justify_center()
        .when(selected, |el| {
            el.bg(bg_row_selected())
                .border_1()
                .border_color(border_selected())
        })
        .when(!selected, |el| {
            el.bg(bg_elevated())
                .border_1()
                .border_color(border_subtle())
                .hover(|s| s.bg(bg_row_hover()))
        })
        .on_click(cx.listener(move |this, _ev, window, cx| {
            this.set_theme(pref, window, cx);
        }))
        .child(Icon::new(icon).text_color(icon_color))
        .child(
            div()
                .text_xs()
                .font_weight(if selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(text_primary())
                .child(title),
        )
        .into_any_element()
}

fn render_language_card(
    id: &'static str,
    lang: AppLanguage,
    locale_code: &'static str,
    selected: bool,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let icon_color = if selected {
        accent_blue()
    } else {
        text_secondary()
    };

    h_flex()
        .id(id)
        .flex_1()
        .px_3p5()
        .py_3()
        .gap_2p5()
        .rounded(px(8.0))
        .cursor_pointer()
        .items_center()
        .justify_between()
        .when(selected, |el| {
            el.bg(bg_row_selected())
                .border_1()
                .border_color(border_selected())
        })
        .when(!selected, |el| {
            el.bg(bg_elevated())
                .border_1()
                .border_color(border_subtle())
                .hover(|s| s.bg(bg_row_hover()))
        })
        .on_click(cx.listener(move |this, _ev, window, cx| {
            this.set_language(lang, window, cx);
        }))
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(IconName::Globe).small().text_color(icon_color))
                .child(
                    div()
                        .text_xs()
                        .font_weight(if selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(text_primary())
                        .child(lang.display_name()),
                ),
        )
        .child(
            div()
                .px_1p5()
                .py_0p5()
                .rounded(px(4.0))
                .bg(bg_panel())
                .text_xs()
                .text_color(text_muted())
                .child(locale_code),
        )
        .into_any_element()
}

fn render_about_settings(
    ws: &RustduckWorkspace,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();
    let (sftp_icon, sftp_col) = protocol_icon_and_color(ProtocolKind::Sftp);
    let (ftp_icon, ftp_col) = protocol_icon_and_color(ProtocolKind::Ftp);
    let (s3_icon, s3_col) = protocol_icon_and_color(ProtocolKind::S3);
    let (smb_icon, smb_col) = protocol_icon_and_color(ProtocolKind::Smb);
    let (webdav_icon, webdav_col) = protocol_icon_and_color(ProtocolKind::WebDav);
    let (gdrive_icon, gdrive_col) = protocol_icon_and_color(ProtocolKind::GoogleDrive);
    let (onedrive_icon, onedrive_col) = protocol_icon_and_color(ProtocolKind::OneDrive);

    let brand_card = h_flex()
        .w_full()
        .p_4()
        .gap_3p5()
        .rounded(px(10.0))
        .bg(bg_elevated())
        .border_1()
        .border_color(border_subtle())
        .items_center()
        .child(img("icons/brand_logo.png").size(px(52.0)))
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::BOLD)
                                .text_color(text_primary())
                                .child("Rustduck"),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded(px(99.0))
                                .bg(bg_panel())
                                .border_1()
                                .border_color(border_subtle())
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(accent_blue())
                                .child(format!("v{APP_VERSION}")),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(text_secondary())
                        .child(t.about_tagline()),
                ),
        );

    let protocols_card = v_flex()
        .w_full()
        .p_3p5()
        .gap_2()
        .rounded(px(10.0))
        .bg(bg_elevated())
        .border_1()
        .border_color(border_subtle())
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(text_secondary())
                .child(t.about_protocols_label()),
        )
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .flex_wrap()
                .child(render_protocol_pill(sftp_icon, sftp_col, t.proto_sftp_label()))
                .child(render_protocol_pill(ftp_icon, ftp_col, t.proto_ftp_label()))
                .child(render_protocol_pill(s3_icon, s3_col, t.proto_s3_label()))
                .child(render_protocol_pill(smb_icon, smb_col, t.proto_smb_label()))
                .child(render_protocol_pill(webdav_icon, webdav_col, t.proto_webdav_label()))
                .child(render_protocol_pill(gdrive_icon, gdrive_col, t.proto_gdrive_label()))
                .child(render_protocol_pill(onedrive_icon, onedrive_col, t.proto_onedrive_label())),
        );

    let is_checking = matches!(ws.update_state, UpdateCheckState::Checking);
    let available_release = match &ws.update_state {
        UpdateCheckState::UpdateAvailable(info) => Some(info.clone()),
        _ => None,
    };
    let (status_icon, status_icon_col, status_text, status_text_col) = match &ws.update_state {
        UpdateCheckState::Idle => (
            IconName::Info,
            accent_blue(),
            t.about_update_idle_desc().to_string(),
            text_muted(),
        ),
        UpdateCheckState::Checking => (
            IconName::RefreshCw,
            accent_amber(),
            t.about_update_checking().to_string(),
            text_secondary(),
        ),
        UpdateCheckState::UpToDate { .. } => (
            IconName::CircleCheck,
            accent_green(),
            t.about_update_up_to_date(APP_VERSION),
            text_secondary(),
        ),
        UpdateCheckState::UpdateAvailable(info) => (
            IconName::Download,
            accent_blue(),
            t.about_update_available(&info.version),
            accent_blue(),
        ),
        UpdateCheckState::Failed(err) => (
            IconName::Info,
            accent_red(),
            t.about_update_failed(err),
            accent_red(),
        ),
    };

    let updates_card = v_flex()
        .w_full()
        .p_3p5()
        .gap_2()
        .rounded(px(10.0))
        .bg(bg_elevated())
        .border_1()
        .border_color(border_subtle())
        .child(
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(text_secondary())
                        .child(t.about_update_section_title()),
                )
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .when_some(available_release, |el, info| {
                            let url = info.html_url.clone();
                            el.child(
                                Button::new("about-download-update-btn")
                                    .xsmall()
                                    .primary()
                                    .icon(IconName::Download)
                                    .label(t.about_download_update_btn(&info.version))
                                    .on_click(cx.listener(move |_this, _ev, _win, _cx| {
                                        RustduckWorkspace::open_url_in_browser(&url);
                                    })),
                            )
                        })
                        .child(
                            Button::new("about-manual-check-update")
                                .xsmall()
                                .secondary()
                                .icon(IconName::RefreshCw)
                                .label(t.about_check_update_btn(is_checking))
                                .disabled(is_checking)
                                .on_click(cx.listener(|this, _ev, _win, cx| {
                                    this.trigger_update_check(true, cx);
                                })),
                        ),
                ),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(status_icon).small().text_color(status_icon_col))
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(status_text_col)
                        .child(status_text),
                ),
        );

    v_flex()
        .w_full()
        .min_w_0()
        .gap_3p5()
        .child(brand_card)
        .child(protocols_card)
        .child(updates_card)
        .into_any_element()
}

fn render_protocol_pill(
    icon: IconName,
    color: gpui::Hsla,
    label: &'static str,
) -> AnyElement {
    h_flex()
        .px_2p5()
        .py_1()
        .gap_1p5()
        .rounded(px(6.0))
        .bg(bg_panel())
        .border_1()
        .border_color(border_subtle())
        .items_center()
        .child(Icon::new(icon).xsmall().text_color(color))
        .child(div().text_xs().text_color(text_primary()).child(label))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_tab_default_and_version_constant() {
        assert_eq!(SettingsTab::default(), SettingsTab::General);
        assert_ne!(SettingsTab::General, SettingsTab::About);
        assert!(!APP_VERSION.is_empty());
    }

    #[test]
    fn test_settings_modal_dimensions_and_content_width_constraints() {
        let prod_src = include_str!("settings_modal.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production source before #[cfg(test)]");
        assert!(
            prod_src.contains("SETTINGS_MODAL_WIDTH_PX: f32 = 820.0"),
            "Settings modal width must be at least 820px to display all options comfortably"
        );
        assert!(
            prod_src.contains("SETTINGS_MODAL_HEIGHT_PX: f32 = 620.0"),
            "Settings modal height must be at least 620px to fit all 4 general sections"
        );
        assert!(
            prod_src.contains(".w(px(SETTINGS_CONTENT_WIDTH_PX))")
                && prod_src.contains(".min_w_0()"),
            "Settings content pane must have explicit width and min_w_0 so flex_wrap pills do not overflow horizontally"
        );
    }
}
