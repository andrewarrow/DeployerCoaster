#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod app_icons;
mod apple;
mod commands;
mod console_cookies;
mod console_sync;
mod dashboard;
mod dynadot;
#[cfg(target_os = "macos")]
mod macos;
mod metadata;
mod play_console;
mod play_store;
mod preferences;
mod storage;
mod style;
mod workspace;

use std::{error::Error, num::NonZeroU32, sync::Arc, time::Instant};

use egui::ViewportId;
use egui_wgpu::winit::Painter;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Icon, Window, WindowId},
};

// Each native window owns its egui input/render state, so keyboard focus, scroll,
// and clipboard actions stay with the window the user is interacting with.
struct DesktopWindow {
    kind: Option<app::AppWindow>,
    context: egui::Context,
    window: Arc<Window>,
    input: egui_winit::State,
    painter: Painter,
    shown: bool,
}

impl DesktopWindow {
    fn new(
        event_loop: &ActiveEventLoop,
        proxy: winit::event_loop::EventLoopProxy<Instant>,
        kind: Option<app::AppWindow>,
        title: &str,
    ) -> Result<Self, Box<dyn Error>> {
        let context = egui::Context::default();
        style::configure(&context);
        context.set_request_repaint_callback(move |request| {
            if let Some(deadline) = Instant::now().checked_add(request.delay) {
                let _ = proxy.send_event(deadline);
            }
        });
        let logo = image::load_from_memory(metadata::LOGO_BYTES)?.into_rgba8();
        let icon = Icon::from_rgba(logo.to_vec(), logo.width(), logo.height())?;
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(title)
                    .with_inner_size(if kind.is_some() {
                        if kind == Some(app::AppWindow::Settings) {
                            LogicalSize::new(820.0, 620.0)
                        } else {
                            LogicalSize::new(640.0, 560.0)
                        }
                    } else {
                        LogicalSize::new(1440.0, 900.0)
                    })
                    .with_min_inner_size(LogicalSize::new(390.0, 360.0))
                    .with_window_icon(Some(icon))
                    .with_visible(false),
            )?,
        );
        let mut painter = pollster::block_on(Painter::new(
            context.clone(),
            egui_wgpu::WgpuConfiguration::default(),
            false,
            egui_wgpu::RendererOptions::default(),
        ));
        pollster::block_on(painter.set_window(ViewportId::ROOT, Some(window.clone())))?;
        let mut input = egui_winit::State::new(
            context.clone(),
            ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            painter.max_texture_side(),
        );
        egui_winit::update_viewport_info(
            input
                .egui_input_mut()
                .viewports
                .entry(ViewportId::ROOT)
                .or_default(),
            &context,
            &window,
            true,
        );
        window.request_redraw();
        Ok(Self {
            kind,
            context,
            window,
            input,
            painter,
            shown: false,
        })
    }
}

impl Drop for DesktopWindow {
    fn drop(&mut self) {
        self.painter.destroy();
    }
}

struct Desktop {
    app: app::App,
    windows: Vec<DesktopWindow>,
    next_repaint: Option<Instant>,
    startup_error: Option<Box<dyn Error>>,
    proxy: winit::event_loop::EventLoopProxy<Instant>,
}

impl Desktop {
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        self.windows.push(DesktopWindow::new(
            event_loop,
            self.proxy.clone(),
            None,
            &self.app.title(),
        )?);
        #[cfg(target_os = "macos")]
        {
            macos::install_native_menu(self.proxy.clone());
            macos::set_application_icon();
        }
        Ok(())
    }

    fn open_app_windows(&mut self, event_loop: &ActiveEventLoop) {
        while let Some(kind) = self.app.take_app_window_request() {
            if let Some(existing) = self.windows.iter().find(|window| window.kind == Some(kind)) {
                existing.window.set_minimized(false);
                existing.window.focus_window();
                existing.window.request_redraw();
                continue;
            }
            match DesktopWindow::new(event_loop, self.proxy.clone(), Some(kind), kind.title()) {
                Ok(window) => self.windows.push(window),
                Err(error) => {
                    log::error!("Could not open {} window: {error}", kind.title());
                    self.app.window_error(kind);
                    self.focus_main_window();
                }
            }
        }
    }

    fn focus_main_window(&self) {
        if let Some(main) = self.windows.iter().find(|window| window.kind.is_none()) {
            main.window.set_minimized(false);
            main.window.focus_window();
            main.window.request_redraw();
        }
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop, index: usize) {
        let desktop = &mut self.windows[index];
        let size = desktop.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        egui_winit::update_viewport_info(
            desktop
                .input
                .egui_input_mut()
                .viewports
                .entry(ViewportId::ROOT)
                .or_default(),
            &desktop.context,
            &desktop.window,
            false,
        );
        let mut focus_settings = false;
        let output = desktop
            .context
            .run_ui(desktop.input.take_egui_input(&desktop.window), |ui| {
                if let Some(kind) = desktop.kind {
                    focus_settings = self.app.app_window_ui(kind, ui);
                } else {
                    self.app.ui(ui);
                }
            });
        #[cfg(target_os = "macos")]
        if desktop.window.has_focus() {
            macos::update_menu_state(
                self.app.native_menu_state(),
                desktop.context.egui_wants_keyboard_input(),
            );
        }
        desktop.input.handle_platform_output_with_event_loop(
            &desktop.window,
            event_loop,
            output.platform_output,
        );
        let primitives = desktop
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        desktop.painter.paint_and_update_textures(
            ViewportId::ROOT,
            output.pixels_per_point,
            desktop
                .context
                .global_style()
                .visuals
                .window_fill()
                .to_normalized_gamma_f32(),
            &primitives,
            &output.textures_delta,
            Vec::new(),
            &desktop.window,
        );
        if desktop.kind.is_none() {
            desktop.window.set_title(&self.app.title());
        }
        if !desktop.shown {
            desktop.window.set_visible(true);
            desktop.shown = true;
        }
        if let Some(deadline) = output
            .viewport_output
            .get(&ViewportId::ROOT)
            .and_then(|viewport| Instant::now().checked_add(viewport.repaint_delay))
        {
            self.next_repaint = Some(
                self.next_repaint
                    .map_or(deadline, |next| next.min(deadline)),
            );
        }
        if focus_settings {
            self.focus_main_window();
        }
        self.open_app_windows(event_loop);
        if self.app.should_quit() {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<Instant> for Desktop {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.windows.is_empty()
            && let Err(error) = self.initialize(event_loop)
        {
            self.startup_error = Some(error);
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, deadline: Instant) {
        self.next_repaint = Some(
            self.next_repaint
                .map_or(deadline, |next| next.min(deadline)),
        );
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(index) = self
            .windows
            .iter()
            .position(|window| window.window.id() == window_id)
        else {
            return;
        };
        let desktop = &mut self.windows[index];
        let response = desktop.input.on_window_event(&desktop.window, &event);
        if response.repaint {
            desktop.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => {
                if desktop.kind.is_some() {
                    self.windows.remove(index);
                } else {
                    self.app.command(commands::Command::Quit);
                    if self.app.should_quit() {
                        event_loop.exit();
                    } else {
                        desktop.window.request_redraw();
                    }
                }
            }
            WindowEvent::DroppedFile(path) if desktop.kind.is_none() => {
                self.app.command(commands::Command::OpenPath(path));
                desktop.window.request_redraw();
            }
            WindowEvent::Resized(size) => {
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    desktop
                        .painter
                        .on_window_resized(ViewportId::ROOT, width, height);
                    desktop.window.request_redraw();
                }
            }
            WindowEvent::Focused(true) => {
                #[cfg(target_os = "macos")]
                macos::update_menu_state(
                    self.app.native_menu_state(),
                    desktop.context.egui_wants_keyboard_input(),
                );
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop, index),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "macos")]
        while let Some(action) = macos::take_menu_action() {
            match action {
                macos::MenuAction::Command(command) => {
                    let state = self.app.native_menu_state();
                    let replaces_workspace = matches!(
                        &command,
                        commands::Command::OpenWorkspace
                            | commands::Command::Save
                            | commands::Command::SaveAs
                            | commands::Command::CloseWorkspace
                            | commands::Command::Quit
                    );
                    if !state.has_pending_action || !replaces_workspace {
                        let focus_main =
                            replaces_workspace || matches!(command, commands::Command::Settings);
                        self.app.command(command);
                        if focus_main {
                            self.focus_main_window();
                        }
                    }
                }
                macos::MenuAction::Edit(edit) => {
                    if let Some(desktop) = self
                        .windows
                        .iter_mut()
                        .find(|window| window.window.has_focus())
                    {
                        let input = &mut desktop.input;
                        match edit {
                            macos::EditAction::Cut => {
                                input.egui_input_mut().events.push(egui::Event::Cut)
                            }
                            macos::EditAction::Copy => {
                                input.egui_input_mut().events.push(egui::Event::Copy)
                            }
                            macos::EditAction::Paste => {
                                if let Some(text) = input.clipboard_text() {
                                    input.egui_input_mut().events.push(egui::Event::Paste(text));
                                }
                            }
                            macos::EditAction::Undo => push_shortcut(input, egui::Key::Z, false),
                            macos::EditAction::Redo => push_shortcut(input, egui::Key::Z, true),
                            macos::EditAction::SelectAll => {
                                push_shortcut(input, egui::Key::A, false)
                            }
                        }
                        desktop.window.request_redraw();
                    }
                }
            }
            macos::update_menu_state(
                self.app.native_menu_state(),
                self.windows
                    .iter()
                    .find(|window| window.window.has_focus())
                    .is_some_and(|window| window.context.egui_wants_keyboard_input()),
            );
            if self.app.should_quit() {
                event_loop.exit();
                return;
            }
            if let Some(window) = self.windows.iter().find(|window| window.kind.is_none()) {
                window.window.request_redraw();
            }
        }
        self.open_app_windows(event_loop);
        if let Some(deadline) = self.next_repaint {
            if deadline <= Instant::now() {
                self.next_repaint = None;
                for desktop in &self.windows {
                    desktop.window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            } else {
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.windows.clear();
    }
}

#[cfg(target_os = "macos")]
fn push_shortcut(input: &mut egui_winit::State, key: egui::Key, shift: bool) {
    let modifiers = egui::Modifiers {
        mac_cmd: true,
        command: true,
        shift,
        ..Default::default()
    };
    for pressed in [true, false] {
        input.egui_input_mut().events.push(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers,
        });
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let mut builder = EventLoop::<Instant>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        builder.with_default_menu(true);
        builder.with_activation_policy(winit::platform::macos::ActivationPolicy::Regular);
    }
    let event_loop = builder.build()?;
    let proxy = event_loop.create_proxy();
    let mut desktop = Desktop {
        app: app::App::new(std::env::args_os().nth(1).map(Into::into)),
        windows: Vec::new(),
        next_repaint: None,
        startup_error: None,
        proxy,
    };
    event_loop.run_app(&mut desktop)?;
    if let Some(error) = desktop.startup_error {
        return Err(error);
    }
    Ok(())
}
