use crate::db::ProjectStat;
use egui_glow::EguiGlow;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasWindowHandle;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowLevel};

pub enum PopupAction {
    None,
    Start { project: String, mins: u64 },
    TogglePause,
    Stop,
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TimerRunState {
    Idle,
    Running,
    Paused,
}

pub struct PopupWindow {
    pub window: Arc<Window>,
    gl_context: PossiblyCurrentContext,
    gl_surface: Surface<WindowSurface>,
    gl: Arc<egui_glow::glow::Context>,
    egui_glow: EguiGlow,
    pub project_input: String,
    pub duration_mins: u64,
    pub recent_projects: Vec<String>,
    pub today_stats: Vec<ProjectStat>,
    shown_at: Option<Instant>,
}

impl PopupWindow {
    pub fn new(event_loop: &ActiveEventLoop) -> Self {
        let window_attrs = Window::default_attributes()
            .with_title("Pomo")
            .with_inner_size(LogicalSize::new(280.0, 235.0))
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_visible(false);

        let template = ConfigTemplateBuilder::new().with_alpha_size(8);
        let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attrs));

        let (window, gl_config) = display_builder
            .build(event_loop, template, |configs| {
                configs
                    .reduce(|accum, config| {
                        if config.num_samples() > accum.num_samples() {
                            config
                        } else {
                            accum
                        }
                    })
                    .unwrap()
            })
            .expect("Failed to build OpenGL window");

        let window = Arc::new(window.unwrap());
        let raw_window_handle = window.window_handle().unwrap().as_raw();
        let gl_display = gl_config.display();

        let context_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::OpenGl(None))
            .build(Some(raw_window_handle));

        let not_current = unsafe {
            gl_display
                .create_context(&gl_config, &context_attrs)
                .expect("Failed to create GL context")
        };

        let size = window.inner_size();
        let surface_attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_window_handle,
            NonZeroU32::new(size.width.max(1)).unwrap(),
            NonZeroU32::new(size.height.max(1)).unwrap(),
        );

        let gl_surface = unsafe {
            gl_display
                .create_window_surface(&gl_config, &surface_attrs)
                .expect("Failed to create GL surface")
        };

        let gl_context = not_current.make_current(&gl_surface).unwrap();
        let gl = Arc::new(unsafe {
            egui_glow::glow::Context::from_loader_function_cstr(|s| gl_display.get_proc_address(s))
        });

        let egui_glow = EguiGlow::new(event_loop, gl.clone(), None, None, true);

        // Load a standard macOS TrueType font so all UI text renders reliably
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(font_bytes) = std::fs::read("/System/Library/Fonts/Supplemental/Arial.ttf") {
            fonts.font_data.insert(
                "arial".to_owned(),
                egui::FontData::from_owned(font_bytes).into(),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "arial".to_owned());
            egui_glow.egui_ctx.set_fonts(fonts);
        }

        Self {
            window,
            gl_context,
            gl_surface,
            gl,
            egui_glow,
            project_input: "general".to_string(),
            duration_mins: 25,
            recent_projects: Vec::new(),
            today_stats: Vec::new(),
            shown_at: None,
        }
    }

    pub fn update_data(&mut self, recent_projects: Vec<String>, today_stats: Vec<ProjectStat>) {
        if let Some(first) = recent_projects.first() {
            if self.project_input == "general" || self.project_input.is_empty() {
                self.project_input = first.clone();
            }
        }
        self.recent_projects = recent_projects;
        self.today_stats = today_stats;
    }

    pub fn show_near_menu_bar(&mut self, x: f64, y: f64) {
        let pos_x = (x - 120.0).max(10.0);
        let pos_y = y + 6.0;
        self.shown_at = Some(Instant::now());
        self.window
            .set_outer_position(LogicalPosition::new(pos_x, pos_y));
        self.window.set_visible(true);
        self.window.focus_window();
        self.window.request_redraw();
    }

    pub fn should_close_on_unfocus(&self) -> bool {
        match self.shown_at {
            Some(t) => t.elapsed() > Duration::from_millis(300),
            None => true,
        }
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    pub fn on_window_event(&mut self, event: &WindowEvent) -> bool {
        let response = self.egui_glow.on_window_event(&self.window, event);
        if response.repaint && self.window.is_visible().unwrap_or(false) {
            self.window.request_redraw();
        }
        response.consumed
    }

    pub fn render(&mut self, run_state: TimerRunState, status_text: &str) -> PopupAction {
        let mut action = PopupAction::None;

        let _ = self.gl_context.make_current(&self.gl_surface);

        self.egui_glow.run(&self.window, |ctx| {
            let frame = egui::Frame::window(&ctx.style()).inner_margin(14.0);

            egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Pomodoro");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("X").clicked() {
                            action = PopupAction::Close;
                        }
                    });
                });

                ui.add_space(4.0);
                let clean_status = status_text
                    .trim_start_matches("🍅 ")
                    .trim_start_matches("⏸ ")
                    .trim();
                ui.label(egui::RichText::new(clean_status).weak());
                ui.separator();
                ui.add_space(4.0);

                // Project Input Box
                ui.horizontal(|ui| {
                    ui.label("Project:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.project_input)
                            .desired_width(170.0)
                            .hint_text("e.g. rust, writing"),
                    );
                });

                // Dynamic Recent Project Chips from SQLite
                if !self.recent_projects.is_empty() {
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for proj in &self.recent_projects {
                            let selected = self.project_input == *proj;
                            if ui.selectable_label(selected, proj).clicked() {
                                self.project_input = proj.clone();
                            }
                        }
                    });
                }

                ui.add_space(6.0);

                // Duration Slider
                ui.horizontal(|ui| {
                    ui.label("Minutes:");
                    ui.add(egui::Slider::new(&mut self.duration_mins, 1..=90));
                });

                ui.add_space(8.0);

                // Action Buttons (Start + Pause/Resume + Stop)
                ui.horizontal(|ui| {
                    let project = if self.project_input.trim().is_empty() {
                        "general".to_string()
                    } else {
                        self.project_input.trim().to_string()
                    };

                    if ui.button("Start").clicked() {
                        action = PopupAction::Start {
                            project,
                            mins: self.duration_mins,
                        };
                    }

                    if run_state != TimerRunState::Idle {
                        let pause_label = if run_state == TimerRunState::Paused {
                            "Resume"
                        } else {
                            "Pause"
                        };
                        if ui.button(pause_label).clicked() {
                            action = PopupAction::TogglePause;
                        }

                        if ui.button("Stop").clicked() {
                            action = PopupAction::Stop;
                        }
                    }
                });

                // Today's SQLite Stats Footer
                ui.add_space(6.0);
                ui.separator();
                if self.today_stats.is_empty() {
                    ui.label(egui::RichText::new("Today: No sessions yet").small().weak());
                } else {
                    let summary: Vec<String> = self
                        .today_stats
                        .iter()
                        .take(3)
                        .map(|s| {
                            if s.total_minutes >= 60 {
                                format!(
                                    "{} ({}h {}m)",
                                    s.project,
                                    s.total_minutes / 60,
                                    s.total_minutes % 60
                                )
                            } else {
                                format!("{} ({}m)", s.project, s.total_minutes)
                            }
                        })
                        .collect();
                    ui.label(
                        egui::RichText::new(format!("Today: {}", summary.join(" • ")))
                            .small()
                            .weak(),
                    );
                }
            });
        });

        unsafe {
            use egui_glow::glow::HasContext;
            self.gl.clear_color(0.1, 0.1, 0.1, 1.0);
            self.gl.clear(egui_glow::glow::COLOR_BUFFER_BIT);
        }

        self.egui_glow.paint(&self.window);
        let _ = self.gl_surface.swap_buffers(&self.gl_context);

        action
    }
}