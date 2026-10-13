//! Rayperclip – clipboard history in the macOS menu bar.

mod app;
mod macos;
mod ui;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(rayperclip::APP_NAME)
            .with_inner_size([460.0, 620.0])
            .with_min_inner_size([320.0, 300.0])
            .with_visible(false),
        // Run as a menu-bar app: no Dock icon and don't steal focus at launch,
        // otherwise Rayperclip would be the "frontmost app" and skip clips.
        event_loop_builder: Some(Box::new(|builder| {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder
                .with_activation_policy(ActivationPolicy::Accessory)
                .with_activate_ignoring_other_apps(false)
                .with_default_menu(false);
        })),
        ..Default::default()
    };
    eframe::run_native(
        rayperclip::APP_NAME,
        options,
        Box::new(|cc| {
            app::start(cc.egui_ctx.clone())?;
            Ok(Box::new(ui::RayperclipApp::default()))
        }),
    )
}
