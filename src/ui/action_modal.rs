use crate::i18n::i18n;
use crate::ui::file_pane::PaneInlineAction;
use crate::ui::workspace::RustduckWorkspace;
use crate::ui::{
    accent_amber, accent_blue, accent_red, bg_elevated, bg_panel, border_strong, border_subtle,
    text_muted, text_primary, text_secondary,
};
use gpui::{
    div, prelude::*, px, AnyElement, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Styled,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::{h_flex, v_flex, Icon, Sizable};
use gpui_kit_assets::IconName;

pub const ACTION_MODAL_WIDTH_PX: f32 = 460.0;

pub fn render_pane_action_modal(
    tab_idx: usize,
    ws: &RustduckWorkspace,
    cx: &mut gpui::Context<RustduckWorkspace>,
) -> Option<AnyElement> {
    let tab = ws.tabs.get(tab_idx)?;
    let (is_local, action) = tab.active_action_modal()?;
    let t = i18n();

    let action_input = if is_local {
        &tab.local_action_input
    } else {
        &tab.remote_action_input
    };
    let parent_path = if is_local {
        &tab.local_path
    } else {
        &tab.remote_path
    };

    let (header_icon, header_icon_color, header_title, body_el, submit_btn) = match action {
        PaneInlineAction::NewFolder => {
            let body = v_flex()
                .w_full()
                .gap_2p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(text_secondary())
                        .child(t.action_modal_new_folder_desc(parent_path)),
                )
                .child(
                    div().w_full().child(
                        Input::new(action_input)
                            .small()
                            .cleanable(true),
                    ),
                )
                .into_any_element();

            let submit = Button::new("action-modal-submit-new-folder")
                .small()
                .primary()
                .icon(IconName::Check)
                .label(t.create_btn())
                .on_click(cx.listener(move |this, _ev, window, cx| {
                    this.submit_inline_action(tab_idx, is_local, window, cx);
                }));

            (
                IconName::FolderPlus,
                accent_blue(),
                t.action_modal_new_folder_title(is_local).to_string(),
                body,
                submit,
            )
        }
        PaneInlineAction::Rename { original_name, .. } => {
            let body = v_flex()
                .w_full()
                .gap_2p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(text_secondary())
                        .child(t.action_modal_rename_desc(&original_name)),
                )
                .child(
                    div().w_full().child(
                        Input::new(action_input)
                            .small()
                            .cleanable(true),
                    ),
                )
                .into_any_element();

            let submit = Button::new("action-modal-submit-rename")
                .small()
                .primary()
                .icon(IconName::Check)
                .label(t.confirm_btn())
                .on_click(cx.listener(move |this, _ev, window, cx| {
                    this.submit_inline_action(tab_idx, is_local, window, cx);
                }));

            (
                IconName::Pencil,
                accent_amber(),
                t.action_modal_rename_title(is_local).to_string(),
                body,
                submit,
            )
        }
        PaneInlineAction::ConfirmDelete { entry } => {
            let prompt = t.inline_delete_confirm_prompt(entry.kind.is_dir_like(), &entry.name);
            let body = render_delete_modal_body(prompt, t.action_modal_delete_warning());
            let submit = Button::new("action-modal-submit-delete")
                .small()
                .danger()
                .icon(IconName::Trash)
                .label(t.confirm_delete_btn())
                .on_click(cx.listener(move |this, _ev, window, cx| {
                    this.submit_inline_action(tab_idx, is_local, window, cx);
                }));

            (
                IconName::Trash,
                accent_red(),
                t.action_modal_delete_title(is_local).to_string(),
                body,
                submit,
            )
        }
        PaneInlineAction::ConfirmBatchDelete { entries } => {
            let summary = t.pane_footer_multi_selection(
                entries.len(),
                entries.first().map(|e| e.name.as_str()),
            );
            let prompt = t.inline_delete_confirm_prompt(false, &summary);
            let body = render_delete_modal_body(prompt, t.action_modal_delete_warning());
            let submit = Button::new("action-modal-submit-batch-delete")
                .small()
                .danger()
                .icon(IconName::Trash)
                .label(t.confirm_delete_btn())
                .on_click(cx.listener(move |this, _ev, window, cx| {
                    this.submit_inline_action(tab_idx, is_local, window, cx);
                }));

            (
                IconName::Trash,
                accent_red(),
                t.action_modal_delete_title(is_local).to_string(),
                body,
                submit,
            )
        }
    };

    let header = h_flex()
        .w_full()
        .h(px(46.0))
        .px_4()
        .bg(bg_elevated())
        .border_b_1()
        .border_color(border_subtle())
        .justify_between()
        .items_center()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(Icon::new(header_icon).small().text_color(header_icon_color))
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(text_primary())
                        .child(header_title),
                ),
        )
        .child(
            Button::new("close-action-modal")
                .xsmall()
                .ghost()
                .icon(IconName::X)
                .on_click(cx.listener(move |this, _ev, _window, cx| {
                    this.cancel_inline_action(tab_idx, is_local, cx);
                })),
        );

    let footer = h_flex()
        .w_full()
        .px_4()
        .py_3()
        .bg(bg_elevated())
        .border_t_1()
        .border_color(border_subtle())
        .justify_end()
        .items_center()
        .gap_2()
        .child(
            Button::new("cancel-action-modal")
                .small()
                .ghost()
                .label(t.cancel_btn())
                .on_click(cx.listener(move |this, _ev, _window, cx| {
                    this.cancel_inline_action(tab_idx, is_local, cx);
                })),
        )
        .child(submit_btn);

    let modal_card = v_flex()
        .id("pane-action-modal-card")
        .w(px(ACTION_MODAL_WIDTH_PX))
        .rounded(px(12.0))
        .bg(bg_panel())
        .border_1()
        .border_color(border_strong())
        .shadow_lg()
        .overflow_hidden()
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_ev, _win, cx| cx.stop_propagation())
        .child(header)
        .child(div().w_full().p_4().child(body_el))
        .child(footer);

    Some(
        div()
            .id("pane-action-modal-backdrop")
            .absolute()
            .inset_0()
            .bg(gpui::hsla(0.0, 0.0, 0.0, 0.52))
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _ev, _window, cx| {
                this.cancel_inline_action(tab_idx, is_local, cx);
            }))
            .child(modal_card)
            .into_any_element(),
    )
}

fn render_delete_modal_body(prompt_text: String, warning_text: &str) -> AnyElement {
    v_flex()
        .w_full()
        .gap_1p5()
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(text_primary())
                .child(prompt_text),
        )
        .child(
            div()
                .text_xs()
                .text_color(text_muted())
                .child(warning_text.to_string()),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::I18n;
    use crate::services::preferences::AppLanguage;

    #[test]
    fn test_action_modal_dimensions_and_bilingual_titles() {
        assert!(
            ACTION_MODAL_WIDTH_PX >= 420.0 && ACTION_MODAL_WIDTH_PX <= 560.0,
            "Action modal width ({ACTION_MODAL_WIDTH_PX}) should be comfortable for folder/file names"
        );

        let zh = I18n::new(AppLanguage::ZhCn);
        let en = I18n::new(AppLanguage::EnUs);

        assert_eq!(zh.action_modal_new_folder_title(true), "新建本地文件夹");
        assert_eq!(zh.action_modal_new_folder_title(false), "新建远程文件夹");
        assert_eq!(en.action_modal_new_folder_title(true), "New Local Folder");
        assert_eq!(en.action_modal_new_folder_title(false), "New Remote Folder");

        assert_eq!(zh.action_modal_rename_title(true), "重命名本地项目");
        assert_eq!(en.action_modal_rename_title(false), "Rename Remote Item");

        assert_eq!(zh.action_modal_delete_title(true), "删除本地项目");
        assert_eq!(en.action_modal_delete_title(false), "Delete Remote Item");
    }
}
