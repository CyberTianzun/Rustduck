use crate::i18n::i18n;
use crate::models::ProtocolKind;
use crate::services::preferences::{AppLanguage, ThemePreference};
use crate::ui::workspace::RustduckWorkspace;
use crate::ui::{
    accent_blue, accent_green, bg_canvas, bg_elevated, bg_panel, bg_row_hover, bg_row_selected,
    border_selected, border_strong, border_subtle, protocol_icon_and_color, text_muted,
    text_primary, text_secondary,
};
use gpui::{
    div, img, prelude::*, px, AnyElement, Context, FontWeight, IntoElement, MouseButton,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::scroll::ScrollableElement as _;
use gpui_component::{h_flex, v_flex, Icon, Sizable as _};
use gpui_kit_assets::IconName;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

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
        .w(px(156.0))
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
        SettingsTab::General => render_general_settings(current_theme, current_lang, cx),
        SettingsTab::About => render_about_settings(),
    };

    let modal_card = v_flex()
        .id("settings-modal-card")
        .w(px(580.0))
        .h(px(396.0))
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
                .overflow_hidden()
                .child(sidebar)
                .child(
                    v_flex()
                        .flex_1()
                        .h_full()
                        .p_5()
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
    current_theme: ThemePreference,
    current_lang: AppLanguage,
    cx: &mut Context<RustduckWorkspace>,
) -> AnyElement {
    let t = i18n();

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

    v_flex()
        .w_full()
        .gap_6()
        .child(theme_section)
        .child(language_section)
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

fn render_about_settings() -> AnyElement {
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

    let updates_card = v_flex()
        .w_full()
        .p_3p5()
        .gap_1p5()
        .rounded(px(10.0))
        .bg(bg_elevated())
        .border_1()
        .border_color(border_subtle())
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(text_secondary())
                .child(t.about_update_section_title()),
        )
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(IconName::CircleCheck).small().text_color(accent_green()))
                .child(
                    div()
                        .text_xs()
                        .text_color(text_muted())
                        .child(t.about_update_placeholder()),
                ),
        );

    v_flex()
        .w_full()
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
}
