#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod commands;
mod metadata;
mod preferences;
mod storage;
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

struct Desktop {
    app: app::App,
    context: egui::Context,
    window: Option<Arc<Window>>,
    input: Option<egui_winit::State>,
    painter: Option<Painter>,
    next_repaint: Option<Instant>,
    startup_error: Option<Box<dyn Error>>,
}

impl Desktop {
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), Box<dyn Error>> {
        let logo = image::load_from_memory(metadata::LOGO_BYTES)?.into_rgba8();
        let icon = Icon::from_rgba(logo.to_vec(), logo.width(), logo.height())?;
        let window = Arc::new(
            event_loop.create_window(
                Window::default_attributes()
                    .with_title(self.app.title())
                    .with_inner_size(LogicalSize::new(1100.0, 720.0))
                    .with_min_inner_size(LogicalSize::new(390.0, 360.0))
                    .with_window_icon(Some(icon))
                    .with_visible(false),
            )?,
        );

        let mut painter = pollster::block_on(Painter::new(
            self.context.clone(),
            egui_wgpu::WgpuConfiguration::default(),
            false,
            egui_wgpu::RendererOptions::default(),
        ));
        pollster::block_on(painter.set_window(ViewportId::ROOT, Some(window.clone())))?;

        let mut input = egui_winit::State::new(
            self.context.clone(),
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
            &self.context,
            &window,
            true,
        );
        self.input = Some(input);
        self.painter = Some(painter);
        window.request_redraw();
        self.window = Some(window);
        Ok(())
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(window), Some(input), Some(painter)) =
            (&self.window, &mut self.input, &mut self.painter)
        else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        egui_winit::update_viewport_info(
            input
                .egui_input_mut()
                .viewports
                .entry(ViewportId::ROOT)
                .or_default(),
            &self.context,
            window,
            false,
        );
        let output = self
            .context
            .run_ui(input.take_egui_input(window), |ui| self.app.ui(ui));
        input.handle_platform_output_with_event_loop(window, event_loop, output.platform_output);
        let primitives = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        painter.paint_and_update_textures(
            ViewportId::ROOT,
            output.pixels_per_point,
            self.context
                .global_style()
                .visuals
                .window_fill()
                .to_normalized_gamma_f32(),
            &primitives,
            &output.textures_delta,
            Vec::new(),
            window,
        );
        window.set_title(&self.app.title());
        window.set_visible(true);

        self.next_repaint = output
            .viewport_output
            .get(&ViewportId::ROOT)
            .and_then(|viewport| Instant::now().checked_add(viewport.repaint_delay));
        if self.app.should_quit() {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler<Instant> for Desktop {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none()
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
        let (Some(window), Some(input)) = (&self.window, &mut self.input) else {
            return;
        };
        if window.id() != window_id {
            return;
        }
        let response = input.on_window_event(window, &event);
        if response.repaint {
            window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => {
                self.app.command(commands::Command::Quit);
                if self.app.should_quit() {
                    event_loop.exit();
                } else {
                    window.request_redraw();
                }
            }
            WindowEvent::DroppedFile(path) => {
                self.app.command(commands::Command::OpenPath(path));
                window.request_redraw();
            }
            WindowEvent::Resized(size) => {
                if let (Some(width), Some(height), Some(painter)) = (
                    NonZeroU32::new(size.width),
                    NonZeroU32::new(size.height),
                    &mut self.painter,
                ) {
                    painter.on_window_resized(ViewportId::ROOT, width, height);
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(deadline) = self.next_repaint {
            if deadline <= Instant::now() {
                self.next_repaint = None;
                if let Some(window) = &self.window {
                    window.request_redraw();
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
        if let Some(painter) = &mut self.painter {
            painter.destroy();
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let mut builder = EventLoop::<Instant>::with_user_event();
    // The in-window File menu routes Quit through the unsaved-workspace guard.
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        builder.with_default_menu(false);
    }
    let event_loop = builder.build()?;
    let proxy = event_loop.create_proxy();
    let context = egui::Context::default();
    context.set_request_repaint_callback(move |request| {
        if request.viewport_id == ViewportId::ROOT
            && let Some(deadline) = Instant::now().checked_add(request.delay)
        {
            let _ = proxy.send_event(deadline);
        }
    });
    let mut desktop = Desktop {
        app: app::App::new(std::env::args_os().nth(1).map(Into::into)),
        context,
        window: None,
        input: None,
        painter: None,
        next_repaint: None,
        startup_error: None,
    };
    event_loop.run_app(&mut desktop)?;
    if let Some(error) = desktop.startup_error {
        return Err(error);
    }
    Ok(())
}
