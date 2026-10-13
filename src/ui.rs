//! The searchable clip window (egui).

use chrono::Utc;
use eframe::egui::{self, Align, Color32, Key, Layout, Modifiers, RichText, Stroke};

use rayperclip::model::{Clip, ClipId};
use rayperclip::presentation;
use rayperclip::theme::{self, Theme};

use crate::app::{Event, UiAction, dispatch, with_core};

struct EditState {
    id: ClipId,
    title: String,
    contents: String,
}

#[derive(Default)]
pub struct RayperclipApp {
    query: String,
    clips: Vec<Clip>,
    loaded_revision: u64,
    loaded_query: String,
    selected: usize,
    scroll_to_selected: bool,
    seen_show_generation: u64,
    focus_search: bool,
    applied_theme: String,
    editing: Option<EditState>,
    recording: bool,
    paste_directly: bool,
    theme: String,
    title_length: usize,
    notice: Option<String>,
}

/// Convert a theme into egui visuals.
pub fn visuals_for(t: &Theme) -> egui::Visuals {
    let c = |rgb: [u8; 3]| Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let mut v = if t.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = c(t.background);
    v.window_fill = c(t.background);
    v.extreme_bg_color = c(t.surface);
    v.faint_bg_color = c(t.surface);
    v.override_text_color = Some(c(t.text));
    v.hyperlink_color = c(t.accent);
    v.selection.bg_fill = c(t.accent).gamma_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0, c(t.text));
    v
}

impl RayperclipApp {
    /// Pull fresh data from the core when something changed.
    fn refresh(&mut self, ctx: &egui::Context) {
        let query = self.query.clone();
        let loaded = self.loaded_revision;
        let same_query = self.loaded_query == query;
        let data = with_core(|core| {
            let clips = (core.revision != loaded || !same_query).then(|| core.query(&query));
            (
                core.revision,
                core.show_generation,
                clips,
                core.settings.clone(),
                core.notice.clone(),
            )
        });
        let Some((revision, show_generation, clips, settings, notice)) = data else {
            // Core busy (e.g. a menu is open); try again shortly.
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return;
        };
        if let Some(clips) = clips {
            self.clips = clips;
            self.loaded_revision = revision;
            self.loaded_query = query;
            self.selected = self.selected.min(self.clips.len().saturating_sub(1));
        }
        if show_generation != self.seen_show_generation {
            self.seen_show_generation = show_generation;
            self.query.clear();
            self.selected = 0;
            self.focus_search = true;
            self.scroll_to_selected = true;
            self.editing = None;
        }
        self.recording = settings.recording;
        self.paste_directly = settings.paste_directly;
        self.title_length = settings.title_length;
        self.theme = settings.theme.clone();
        self.notice = notice;
        if self.applied_theme != settings.theme {
            let t = theme::by_name(&settings.theme);
            let pref = if t.dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            };
            ctx.set_theme(pref);
            ctx.set_visuals_of(pref, visuals_for(t));
            self.applied_theme = settings.theme;
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context, actions: &mut Vec<UiAction>) {
        if self.editing.is_some() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.editing = None;
            }
            return;
        }
        let n = self.clips.len();
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) && n > 0 {
                self.selected = (self.selected + 1).min(n - 1);
                self.scroll_to_selected = true;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                self.selected = self.selected.saturating_sub(1);
                self.scroll_to_selected = true;
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                actions.push(UiAction::Hide);
            }
            if i.consume_key(Modifiers::NONE, Key::Enter)
                && let Some(c) = self.clips.get(self.selected)
            {
                actions.push(UiAction::Paste(c.id));
            }
            if i.consume_key(Modifiers::COMMAND, Key::P)
                && let Some(c) = self.clips.get(self.selected)
            {
                actions.push(UiAction::TogglePin(c.id));
            }
            if i.consume_key(Modifiers::COMMAND, Key::Backspace)
                && let Some(c) = self.clips.get(self.selected)
            {
                actions.push(UiAction::Delete(c.id));
            }
            if i.consume_key(Modifiers::COMMAND, Key::E)
                && let Some(c) = self.clips.get(self.selected)
            {
                self.editing = Some(EditState {
                    id: c.id,
                    title: c.title.clone().unwrap_or_default(),
                    contents: c.contents.clone(),
                });
            }
            let digits = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
                Key::Num9,
            ];
            for (idx, key) in digits.into_iter().enumerate() {
                if i.consume_key(Modifiers::COMMAND, key)
                    && let Some(c) = self.clips.get(idx)
                {
                    actions.push(UiAction::Paste(c.id));
                }
            }
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("Search clips…")
                .desired_width(f32::INFINITY),
        );
        if self.focus_search {
            resp.request_focus();
            self.focus_search = false;
        }
        if resp.changed() {
            self.selected = 0;
            self.scroll_to_selected = true;
        }
        ui.add_space(4.0);
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        ui.add_space(4.0);
        if let Some(n) = &self.notice {
            ui.label(RichText::new(n).small());
        }
        ui.horizontal(|ui| {
            let mut rec = self.recording;
            if ui.checkbox(&mut rec, "Record").changed() {
                actions.push(UiAction::SetRecording(rec));
            }
            let mut pd = self.paste_directly;
            if ui
                .checkbox(&mut pd, "Paste directly")
                .on_hover_text("Paste into the frontmost app after choosing a clip (needs Accessibility permission)")
                .changed()
            {
                actions.push(UiAction::SetPasteDirectly(pd));
            }
            let mut selected_theme = theme::by_name(&self.theme).name.to_string();
            egui::ComboBox::from_id_salt("theme")
                .selected_text(&selected_theme)
                .show_ui(ui, |ui| {
                    for t in theme::THEMES.iter() {
                        ui.selectable_value(&mut selected_theme, t.name.to_string(), t.name);
                    }
                });
            if selected_theme != theme::by_name(&self.theme).name {
                actions.push(UiAction::SetTheme(selected_theme));
            }
        });
        ui.horizontal(|ui| {
            ui.weak(format!("{} clips", self.clips.len()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Clear unpinned").clicked() {
                    actions.push(UiAction::ClearUnpinned);
                }
                if !self.paste_directly {
                    return;
                }
                if ui
                    .small_button("Accessibility…")
                    .on_hover_text("Ask macOS for permission to paste automatically")
                    .clicked()
                {
                    actions.push(UiAction::RequestAccessibility);
                }
            });
        });
        ui.add_space(4.0);
    }

    fn clip_list(&mut self, ui: &mut egui::Ui, actions: &mut Vec<UiAction>) {
        if self.clips.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.weak(if self.query.trim().is_empty() {
                    "Copy something to start your history."
                } else {
                    "No results"
                });
            });
            return;
        }
        let now = Utc::now();
        let pinned_count = self.clips.iter().filter(|c| c.pinned).count();
        let scroll = std::mem::take(&mut self.scroll_to_selected);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, clip) in self.clips.iter().enumerate() {
                    if i == 0 && pinned_count > 0 {
                        ui.label(RichText::new("Pinned").small().strong());
                    }
                    if i == pinned_count && pinned_count > 0 {
                        ui.separator();
                        ui.label(RichText::new("Recent").small().strong());
                    }
                    let selected = i == self.selected;
                    let shortcut = if i < 9 {
                        format!("⌘{}  ", i + 1)
                    } else {
                        String::new()
                    };
                    let title = format!("{shortcut}{}", clip.display_title(self.title_length));
                    ui.horizontal(|ui| {
                        let resp = ui
                            .selectable_label(selected, title)
                            .on_hover_text(preview(&clip.contents));
                        if selected && scroll {
                            resp.scroll_to_me(Some(Align::Center));
                        }
                        if resp.clicked() {
                            self.selected = i;
                        }
                        if resp.double_clicked() {
                            actions.push(UiAction::Paste(clip.id));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("✕").on_hover_text("Delete (⌘⌫)").clicked() {
                                actions.push(UiAction::Delete(clip.id));
                            }
                            if ui.small_button("Edit").on_hover_text("Edit (⌘E)").clicked() {
                                self.editing = Some(EditState {
                                    id: clip.id,
                                    title: clip.title.clone().unwrap_or_default(),
                                    contents: clip.contents.clone(),
                                });
                            }
                            let pin_label = if clip.pinned { "Unpin" } else { "Pin" };
                            if ui.small_button(pin_label).on_hover_text("⌘P").clicked() {
                                actions.push(UiAction::TogglePin(clip.id));
                            }
                            if clip.pinned {
                                if i + 1 < pinned_count && ui.small_button("↓").clicked() {
                                    actions.push(UiAction::MovePinned(clip.id, i + 1));
                                }
                                if i > 0 && ui.small_button("↑").clicked() {
                                    actions.push(UiAction::MovePinned(clip.id, i - 1));
                                }
                            }
                        });
                    });
                    ui.label(
                        RichText::new(presentation::clip_subtitle(clip, now))
                            .small()
                            .weak(),
                    );
                    ui.add_space(2.0);
                }
            });
    }

    fn edit_window(&mut self, ctx: &egui::Context, actions: &mut Vec<UiAction>) {
        let Some(edit) = &mut self.editing else {
            return;
        };
        let mut close = false;
        egui::Window::new("Edit clip")
            .collapsible(false)
            .resizable(true)
            .default_width(400.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Title (optional)");
                ui.add(egui::TextEdit::singleline(&mut edit.title).desired_width(f32::INFINITY));
                ui.label("Contents");
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut edit.contents)
                                .desired_rows(10)
                                .desired_width(f32::INFINITY),
                        );
                    });
                ui.horizontal(|ui| {
                    let can_save = !edit.contents.is_empty();
                    if ui
                        .add_enabled(can_save, egui::Button::new("Save"))
                        .clicked()
                    {
                        actions.push(UiAction::SaveEdit {
                            id: edit.id,
                            title: edit.title.clone(),
                            contents: edit.contents.clone(),
                        });
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.editing = None;
        }
    }
}

/// Tooltip preview: at most 20 lines / 1000 characters.
fn preview(contents: &str) -> String {
    let mut s: String = contents.lines().take(20).collect::<Vec<_>>().join("\n");
    if s.chars().count() > 1000 {
        s = s.chars().take(1000).collect();
        s.push('…');
    }
    s
}

impl eframe::App for RayperclipApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.refresh(&ctx);

        let mut actions = Vec::new();
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            actions.push(UiAction::Hide);
        }
        self.handle_keys(&ctx, &mut actions);

        egui::Panel::top("search").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("controls").show(ui, |ui| self.bottom_bar(ui, &mut actions));
        egui::CentralPanel::default().show(ui, |ui| self.clip_list(ui, &mut actions));
        self.edit_window(&ctx, &mut actions);

        for a in actions {
            dispatch(Event::Ui(a));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_limits_lines_and_length() {
        let many = (0..50)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(preview(&many).lines().count(), 20);
        let long = "x".repeat(2000);
        assert_eq!(preview(&long).chars().count(), 1001);
    }

    #[test]
    fn visuals_follow_theme() {
        for t in theme::THEMES.iter() {
            let v = visuals_for(t);
            assert_eq!(v.dark_mode, t.dark);
            let [r, g, b] = t.background;
            assert_eq!(v.panel_fill, Color32::from_rgb(r, g, b));
        }
    }
}
