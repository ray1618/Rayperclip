//! Application core: owns all state and handles events on the main thread.
//!
//! Every native callback (run-loop timer, global hotkeys, menu clicks, UI
//! actions) is funnelled through [`dispatch`]. Events are queued and handled
//! one at a time while the core is borrowed; side effects that can re-enter
//! the event loop (showing windows, popup menus) are executed afterwards,
//! when the core is no longer borrowed.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use core_foundation::runloop::{CFRunLoopTimer, CFRunLoopTimerRef};
use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSRunningApplication;
use tray_icon::menu::{
    CheckMenuItem, ContextMenu, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use rayperclip::clipboard::Clipboard;
use rayperclip::model::{Clip, ClipId};
use rayperclip::paste::{self, PasteOutcome, PasteStep};
use rayperclip::poller::{POLL_INTERVAL_SECS, PollResult, Poller};
use rayperclip::settings::{Paths, Settings};
use rayperclip::store::Store;
use rayperclip::{hotkeys, login, presentation, theme};

use crate::macos::{self, MacClipboard, MacPasteBackend};

/// Something that happened and must be handled by the core.
#[derive(Debug, Clone)]
pub enum Event {
    Tick,
    Menu(MenuId),
    Hotkey(u32),
    Ui(UiAction),
}

/// Actions triggered from the egui window.
#[derive(Debug, Clone)]
pub enum UiAction {
    Paste(ClipId),
    TogglePin(ClipId),
    MovePinned(ClipId, usize),
    Delete(ClipId),
    SaveEdit {
        id: ClipId,
        title: String,
        contents: String,
    },
    ClearUnpinned,
    SetRecording(bool),
    SetPasteDirectly(bool),
    SetTheme(String),
    Hide,
    RequestAccessibility,
}

/// Actions behind tray / popup menu items.
#[derive(Debug, Clone)]
enum MenuAction {
    Paste(ClipId),
    ShowWindow,
    ToggleRecording,
    TogglePasteDirectly,
    SetTheme(&'static str),
    ToggleLogin,
    ClearUnpinned,
    EditSettings,
    ReloadSettings,
    Quit,
}

/// Side effects executed after the core borrow is released.
enum Effect {
    ShowWindow,
    HideWindow(Option<Retained<NSRunningApplication>>),
    Paste {
        clip: Clip,
        steps: Vec<PasteStep>,
        restore_focus: bool,
        restore_to: Option<Retained<NSRunningApplication>>,
    },
    Popup(Menu),
    OpenFile(PathBuf),
    Quit,
}

/// Where a paste request came from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Window,
    Menu,
}

pub struct Core {
    mtm: MainThreadMarker,
    pub store: Store,
    pub settings: Settings,
    paths: Paths,
    poller: Poller,
    clipboard: MacClipboard,
    tray: Option<TrayIcon>,
    tray_actions: HashMap<MenuId, MenuAction>,
    popup_actions: HashMap<MenuId, MenuAction>,
    hotkey_manager: Option<GlobalHotKeyManager>,
    window_hotkey: Option<HotKey>,
    menu_hotkey: Option<HotKey>,
    egui_ctx: egui::Context,
    previous_app: Option<Retained<NSRunningApplication>>,
    _timer: Option<CFRunLoopTimer>,
    /// Bumped whenever clips or settings change; the UI re-queries then.
    pub revision: u64,
    /// Bumped each time the window is shown (UI resets search/selection).
    pub show_generation: u64,
    /// Status message for the UI.
    pub notice: Option<String>,
}

thread_local! {
    static CORE: RefCell<Option<Core>> = const { RefCell::new(None) };
    static QUEUE: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
    static PUMPING: Cell<bool> = const { Cell::new(false) };
}

/// Run `f` with the core, if it exists and is not already borrowed.
pub fn with_core<R>(f: impl FnOnce(&mut Core) -> R) -> Option<R> {
    CORE.with(|c| {
        let mut guard = c.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

/// Queue an event and process the queue unless already processing.
pub fn dispatch(event: Event) {
    QUEUE.with(|q| q.borrow_mut().push_back(event));
    if PUMPING.with(|p| p.replace(true)) {
        return;
    }
    while let Some(ev) = QUEUE.with(|q| q.borrow_mut().pop_front()) {
        let effects = with_core(|c| c.handle(ev)).unwrap_or_default();
        for effect in effects {
            run_effect(effect);
        }
    }
    PUMPING.with(|p| p.set(false));
}

extern "C" fn timer_fired(_timer: CFRunLoopTimerRef, _info: *mut c_void) {
    dispatch(Event::Tick);
}

/// Create the core, tray icon, hotkeys and poll timer. Must be called on
/// the main thread after the event loop started.
pub fn start(egui_ctx: egui::Context) -> Result<()> {
    let mtm = MainThreadMarker::new().context("Rayperclip must start on the main thread")?;
    macos::set_accessory(mtm);

    let paths = Paths::standard().context("cannot determine Application Support directory")?;
    let settings = match Settings::load(&paths.settings) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Rayperclip: {e}; using defaults");
            Settings::default()
        }
    };
    if !paths.settings.exists() {
        settings.save(&paths.settings).ok();
    }
    let store = Store::open(&paths.database)
        .with_context(|| format!("opening {}", paths.database.display()))?;

    let clipboard = MacClipboard::new();
    let poller = Poller::new(macos::own_pid(), store.latest_contents()?);

    let mut core = Core {
        mtm,
        store,
        settings,
        paths,
        poller,
        clipboard,
        tray: None,
        tray_actions: HashMap::new(),
        popup_actions: HashMap::new(),
        hotkey_manager: GlobalHotKeyManager::new()
            .map_err(|e| eprintln!("Rayperclip: hotkeys unavailable: {e}"))
            .ok(),
        window_hotkey: None,
        menu_hotkey: None,
        egui_ctx,
        previous_app: None,
        _timer: None,
        revision: 1,
        show_generation: 0,
        notice: None,
    };
    core.sync_login_item();
    core.register_hotkeys();
    core.create_tray()?;
    core._timer = Some(macos::schedule_repeating(POLL_INTERVAL_SECS, timer_fired));
    CORE.with(|c| *c.borrow_mut() = Some(core));

    MenuEvent::set_event_handler(Some(|e: MenuEvent| dispatch(Event::Menu(e.id))));
    GlobalHotKeyEvent::set_event_handler(Some(|e: GlobalHotKeyEvent| {
        if e.state == HotKeyState::Pressed {
            dispatch(Event::Hotkey(e.id));
        }
    }));
    // Record whatever is on the pasteboard right away.
    dispatch(Event::Tick);
    Ok(())
}

fn run_effect(effect: Effect) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    match effect {
        Effect::ShowWindow => macos::show_window(mtm),
        Effect::HideWindow(prev) => macos::hide_window(mtm, prev.as_deref()),
        Effect::Paste {
            clip,
            steps,
            restore_focus,
            restore_to,
        } => {
            let mut backend = MacPasteBackend {
                mtm,
                restore_to,
                restore: restore_focus,
            };
            paste::execute(&mut backend, &clip, &steps);
        }
        Effect::Popup(menu) => {
            if let Some(view) = macos::content_view_ptr(mtm) {
                // SAFETY: the view belongs to our main window, which lives for
                // the whole process. Popping up runs a nested event loop; the
                // core is not borrowed here.
                unsafe {
                    menu.show_context_menu_for_nsview(view, None);
                }
            }
        }
        Effect::OpenFile(path) => macos::open_in_text_editor(&path),
        Effect::Quit => std::process::exit(0),
    }
}

impl Core {
    fn handle(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Tick => {
                self.poll();
                vec![]
            }
            Event::Hotkey(id) => self.on_hotkey(id),
            Event::Menu(id) => {
                let action = self
                    .tray_actions
                    .get(&id)
                    .or_else(|| self.popup_actions.get(&id))
                    .cloned();
                match action {
                    Some(a) => self.on_menu(a),
                    None => vec![],
                }
            }
            Event::Ui(action) => self.on_ui(action),
        }
    }

    fn changed(&mut self) {
        self.revision += 1;
        self.rebuild_tray_menu();
        self.egui_ctx.request_repaint();
    }

    fn set_notice(&mut self, msg: impl Into<String>) {
        self.notice = Some(msg.into());
        self.egui_ctx.request_repaint();
    }

    fn save_settings(&mut self) {
        if let Err(e) = self.settings.save(&self.paths.settings) {
            self.set_notice(format!("Could not save settings: {e}"));
        }
    }

    // -- polling -----------------------------------------------------------

    fn poll(&mut self) {
        // Cheap check first so we only query the frontmost app on changes.
        if !self.settings.recording {
            self.poller.sync_change_count(self.clipboard.change_count());
            return;
        }
        let front = macos::frontmost_app();
        match self.poller.tick(
            &self.clipboard,
            front.as_ref(),
            &self.settings,
            &mut self.store,
        ) {
            Ok(PollResult::Recorded { .. }) => self.changed(),
            Ok(r) => {
                if std::env::var_os("RAYPERCLIP_DEBUG").is_some() {
                    eprintln!("poll: {r:?} front={front:?}");
                }
            }
            Err(e) => eprintln!("Rayperclip: failed to record clip: {e}"),
        }
    }

    // -- hotkeys -------------------------------------------------------------

    fn register_hotkeys(&mut self) {
        let Some(manager) = &self.hotkey_manager else {
            return;
        };
        for hk in [self.window_hotkey.take(), self.menu_hotkey.take()]
            .into_iter()
            .flatten()
        {
            manager.unregister(hk).ok();
        }
        let mut errors = Vec::new();
        let mut register = |spec: &str, what: &str| -> Option<HotKey> {
            match hotkeys::parse_optional(spec) {
                Ok(Some(hk)) => match manager.register(hk) {
                    Ok(()) => Some(hk),
                    Err(e) => {
                        errors.push(format!("{what} hotkey: {e}"));
                        None
                    }
                },
                Ok(None) => None,
                Err(e) => {
                    errors.push(format!("{what} hotkey: {e}"));
                    None
                }
            }
        };
        let window = register(&self.settings.window_hotkey.clone(), "Window");
        let menu = register(&self.settings.menu_hotkey.clone(), "Menu");
        self.window_hotkey = window;
        self.menu_hotkey = menu;
        if !errors.is_empty() {
            self.set_notice(errors.join("; "));
        }
    }

    fn on_hotkey(&mut self, id: u32) -> Vec<Effect> {
        if self.window_hotkey.is_some_and(|h| h.id() == id) {
            if macos::window_is_visible(self.mtm) {
                return vec![Effect::HideWindow(self.previous_app.take())];
            }
            self.remember_previous_app();
            self.show_generation += 1;
            self.egui_ctx.request_repaint();
            return vec![Effect::ShowWindow];
        }
        if self.menu_hotkey.is_some_and(|h| h.id() == id) {
            let (menu, actions) = self.build_menu(false);
            self.popup_actions = actions;
            return vec![Effect::Popup(menu)];
        }
        vec![]
    }

    fn remember_previous_app(&mut self) {
        self.previous_app =
            macos::frontmost_running_app().filter(|a| a.processIdentifier() != macos::own_pid());
    }

    // -- paste ----------------------------------------------------------------

    fn paste(&mut self, id: ClipId, source: Source) -> Vec<Effect> {
        let clip = match self.store.get(id) {
            Ok(Some(c)) => c,
            _ => {
                self.set_notice("That clip no longer exists.");
                return vec![];
            }
        };
        self.store.increment_paste_count(id).ok();
        self.poller.note_pasted(&clip.contents);
        let (steps, outcome) =
            paste::plan(self.settings.paste_directly, macos::accessibility_trusted());
        match outcome {
            PasteOutcome::Pasted => self.notice = None,
            PasteOutcome::Copied => self.set_notice("Copied to the clipboard."),
            PasteOutcome::CopiedNeedsPermission => self.set_notice(
                "Copied. Grant Accessibility permission to paste automatically \
                 (System Settings → Privacy & Security → Accessibility).",
            ),
        }
        self.changed();
        let from_window = source == Source::Window;
        vec![Effect::Paste {
            clip,
            steps,
            restore_focus: from_window,
            restore_to: if from_window {
                self.previous_app.take()
            } else {
                None
            },
        }]
    }

    // -- menu -----------------------------------------------------------------

    fn on_menu(&mut self, action: MenuAction) -> Vec<Effect> {
        match action {
            MenuAction::Paste(id) => return self.paste(id, Source::Menu),
            MenuAction::ShowWindow => {
                self.previous_app = None;
                self.show_generation += 1;
                self.egui_ctx.request_repaint();
                return vec![Effect::ShowWindow];
            }
            MenuAction::ToggleRecording => self.set_recording(!self.settings.recording),
            MenuAction::TogglePasteDirectly => {
                self.set_paste_directly(!self.settings.paste_directly)
            }
            MenuAction::SetTheme(name) => self.set_theme(name),
            MenuAction::ToggleLogin => {
                self.settings.start_at_login = !self.settings.start_at_login;
                self.save_settings();
                self.sync_login_item();
                self.changed();
            }
            MenuAction::ClearUnpinned => self.clear_unpinned(),
            MenuAction::EditSettings => {
                self.save_settings();
                return vec![Effect::OpenFile(self.paths.settings.clone())];
            }
            MenuAction::ReloadSettings => self.reload_settings(),
            MenuAction::Quit => return vec![Effect::Quit],
        }
        vec![]
    }

    fn set_recording(&mut self, on: bool) {
        self.settings.recording = on;
        if on {
            // Don't record what was copied while recording was off.
            self.poller.sync_change_count(self.clipboard.change_count());
        }
        self.save_settings();
        self.changed();
    }

    fn set_paste_directly(&mut self, on: bool) {
        self.settings.paste_directly = on;
        self.save_settings();
        self.changed();
        if on && !macos::accessibility_trusted() {
            macos::request_accessibility();
        }
    }

    fn set_theme(&mut self, name: &str) {
        self.settings.theme = theme::by_name(name).name.to_string();
        self.save_settings();
        self.changed();
    }

    fn clear_unpinned(&mut self) {
        match self.store.delete_all_unpinned() {
            Ok(n) => self.set_notice(format!("Deleted {n} clips.")),
            Err(e) => self.set_notice(format!("Could not delete clips: {e}")),
        }
        self.poller
            .reset_last_clip(self.store.latest_contents().ok().flatten());
        self.changed();
    }

    fn reload_settings(&mut self) {
        match Settings::load(&self.paths.settings) {
            Ok(s) => {
                self.settings = s;
                self.register_hotkeys();
                self.sync_login_item();
                self.store.trim(self.settings.max_clips).ok();
                if self.notice.is_none() {
                    self.set_notice("Settings reloaded.");
                }
                self.changed();
            }
            Err(e) => self.set_notice(format!("Settings not reloaded: {e}")),
        }
    }

    fn sync_login_item(&mut self) {
        let (Some(dir), Ok(exe)) = (login::launch_agents_dir(), std::env::current_exe()) else {
            return;
        };
        if login::is_enabled(&dir) != self.settings.start_at_login
            && let Err(e) = login::apply(&dir, &exe, self.settings.start_at_login)
        {
            self.set_notice(format!("Could not update login item: {e}"));
        }
    }

    // -- UI actions -----------------------------------------------------------

    fn on_ui(&mut self, action: UiAction) -> Vec<Effect> {
        let result: Result<(), rayperclip::store::StoreError> = match action {
            UiAction::Paste(id) => return self.paste(id, Source::Window),
            UiAction::Hide => return vec![Effect::HideWindow(self.previous_app.take())],
            UiAction::TogglePin(id) => match self.store.get(id) {
                Ok(Some(c)) => self.store.set_pinned(id, !c.pinned),
                Ok(None) => Ok(()),
                Err(e) => Err(e),
            },
            UiAction::MovePinned(id, idx) => self.store.move_pinned(id, idx),
            UiAction::Delete(id) => {
                let r = self.store.delete(id);
                self.poller
                    .reset_last_clip(self.store.latest_contents().ok().flatten());
                r
            }
            UiAction::SaveEdit {
                id,
                title,
                contents,
            } => self
                .store
                .update_contents(id, &contents)
                .and_then(|()| self.store.set_title(id, Some(&title))),
            UiAction::ClearUnpinned => {
                self.clear_unpinned();
                Ok(())
            }
            UiAction::SetRecording(on) => {
                self.set_recording(on);
                Ok(())
            }
            UiAction::SetPasteDirectly(on) => {
                self.set_paste_directly(on);
                Ok(())
            }
            UiAction::SetTheme(name) => {
                self.set_theme(&name);
                Ok(())
            }
            UiAction::RequestAccessibility => {
                macos::request_accessibility();
                Ok(())
            }
        };
        if let Err(e) = result {
            self.set_notice(e.to_string());
        }
        self.changed();
        vec![]
    }

    // -- tray -----------------------------------------------------------------

    fn create_tray(&mut self) -> Result<()> {
        let size = 36;
        let icon = Icon::from_rgba(presentation::tray_icon_rgba(size), size, size)?;
        let (menu, actions) = self.build_menu(true);
        self.tray_actions = actions;
        let tray = TrayIconBuilder::new()
            .with_icon_templated(icon)
            .with_tooltip(rayperclip::APP_NAME)
            .with_menu(Box::new(menu))
            .build()?;
        self.tray = Some(tray);
        Ok(())
    }

    fn rebuild_tray_menu(&mut self) {
        let (menu, actions) = self.build_menu(true);
        if let Some(tray) = &self.tray {
            tray.set_menu(Some(Box::new(menu)));
            self.tray_actions = actions;
        }
    }

    /// Build the clip menu. `full` adds the controls shown in the menu bar;
    /// the hotkey popup only lists clips.
    fn build_menu(&self, full: bool) -> (Menu, HashMap<MenuId, MenuAction>) {
        let menu = Menu::new();
        let mut actions = HashMap::new();
        let max = self.settings.title_length;

        let mut add_item = |menu: &Menu, text: &str, enabled: bool, action: Option<MenuAction>| {
            let item = MenuItem::new(text, enabled, None);
            if let Some(a) = action {
                actions.insert(item.id().clone(), a);
            }
            menu.append(&item).ok();
        };

        let pinned = self.store.pinned().unwrap_or_default();
        let recent = self
            .store
            .recent(self.settings.menu_clip_count)
            .unwrap_or_default();

        if !pinned.is_empty() {
            add_item(&menu, "Pinned", false, None);
            for c in &pinned {
                add_item(
                    &menu,
                    &format!("  {}", c.display_title(max)),
                    true,
                    Some(MenuAction::Paste(c.id)),
                );
            }
            menu.append(&PredefinedMenuItem::separator()).ok();
        }
        if recent.is_empty() {
            add_item(&menu, "No clips yet", false, None);
        } else {
            for (i, c) in recent.iter().enumerate() {
                add_item(
                    &menu,
                    &presentation::menu_label(i, c, max),
                    true,
                    Some(MenuAction::Paste(c.id)),
                );
            }
        }

        if full {
            menu.append(&PredefinedMenuItem::separator()).ok();
            let open_label = match hotkeys::parse_optional(&self.settings.window_hotkey) {
                Ok(Some(hk)) => format!("Open Rayperclip…  {}", hotkeys::display(&hk)),
                _ => "Open Rayperclip…".to_string(),
            };
            add_item(&menu, &open_label, true, Some(MenuAction::ShowWindow));

            let mut check = |text: &str, checked: bool, action: MenuAction| {
                let item = CheckMenuItem::new(text, true, checked, None);
                actions.insert(item.id().clone(), action);
                menu.append(&item).ok();
            };
            check(
                "Record Clipboard",
                self.settings.recording,
                MenuAction::ToggleRecording,
            );
            check(
                "Paste Directly",
                self.settings.paste_directly,
                MenuAction::TogglePasteDirectly,
            );
            check(
                "Start at Login",
                self.settings.start_at_login,
                MenuAction::ToggleLogin,
            );

            let themes = Submenu::new("Theme", true);
            let current = theme::by_name(&self.settings.theme).name;
            for t in theme::THEMES.iter() {
                let item = CheckMenuItem::new(t.name, true, t.name == current, None);
                actions.insert(item.id().clone(), MenuAction::SetTheme(t.name));
                themes.append(&item).ok();
            }
            menu.append(&themes).ok();

            menu.append(&PredefinedMenuItem::separator()).ok();
            let mut plain = |text: &str, action: MenuAction| {
                let item = MenuItem::new(text, true, None);
                actions.insert(item.id().clone(), action);
                menu.append(&item).ok();
            };
            plain("Delete All Unpinned Clips", MenuAction::ClearUnpinned);
            plain("Edit Settings File…", MenuAction::EditSettings);
            plain("Reload Settings", MenuAction::ReloadSettings);
            menu.append(&PredefinedMenuItem::separator()).ok();
            plain("Quit Rayperclip", MenuAction::Quit);
        }
        (menu, actions)
    }

    /// Clips for the UI list: search results (pinned first).
    pub fn query(&self, text: &str) -> Vec<Clip> {
        self.store.search(text, 1000).unwrap_or_default()
    }
}
