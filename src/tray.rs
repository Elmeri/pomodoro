use crate::db::Database;
use crate::popup::{PopupAction, PopupWindow, TimerRunState};
use anyhow::Result;
use notify_rust::Notification;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

#[cfg(target_os = "macos")]
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};

#[derive(Debug)]
enum UserEvent {
    Tick {
        detailed_title: String,
        is_paused: bool,
    },
    Finished {
        project: String,
        mins: u64,
    },
    MenuClicked(MenuId),
}

enum ActiveSession {
    Running {
        end_time: Instant,
        project: String,
        total_mins: u64,
    },
    Paused {
        remaining: Duration,
        project: String,
        total_mins: u64,
    },
}

struct TimerState {
    session: Option<ActiveSession>,
}

struct PomoTrayApp {
    tray_icon: Option<TrayIcon>,
    popup: Option<PopupWindow>,
    status_menu_item: MenuItem,
    show_timer_in_bar_item: CheckMenuItem,
    custom_window_item: MenuItem,
    pause_resume_item: MenuItem,
    stop_item: MenuItem,
    quit_item: MenuItem,
    quick_project_items: HashMap<MenuId, String>,
    state: Arc<Mutex<TimerState>>,
    db: Database,
    current_status: String,
    is_paused: bool,
}

/// Generates a 36x36 (2x Retina 18pt) monochrome template silhouette matching
/// the SVG app logo: filled tomato body, 3-leaf calyx + stem, and negative-space
/// timer ticks along the bottom curve.
fn build_template_tomato_icon() -> Icon {
    let size: u32 = 36;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let cx = 18.0_f32;
    let cy = 21.2_f32;
    let rx = 13.5_f32;
    let ry = 11.4_f32;

    let dist_to_segment = |px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32| -> f32 {
        let vx = x2 - x1;
        let vy = y2 - y1;
        let wx = px - x1;
        let wy = py - y1;
        let len_sq = vx * vx + vy * vy;
        let t = if len_sq > 0.0 {
            (wx * vx + wy * vy) / len_sq
        } else {
            0.0
        }
        .clamp(0.0, 1.0);
        let proj_x = x1 + t * vx;
        let proj_y = y1 + t * vy;
        ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
    };

    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;

            // 1. Solid filled tomato body
            let dx = (fx - cx) / rx;
            let dy = (fy - cy) / ry;
            let body_dist = (dx * dx + dy * dy).sqrt();
            let mut alpha = ((1.03 - body_dist) * 14.0).clamp(0.0, 1.0);

            // 2. Calyx leaves + stem
            let stem_d = dist_to_segment(fx, fy, cx, 4.2, cx, 10.5);
            let stem_alpha = ((1.55 - stem_d) * 2.2).clamp(0.0, 1.0);

            let left_leaf_d = dist_to_segment(fx, fy, cx, 10.2, 9.8, 12.2);
            let right_leaf_d = dist_to_segment(fx, fy, cx, 10.2, 26.2, 12.2);
            let center_leaf_d = dist_to_segment(fx, fy, cx, 10.2, cx, 14.2);

            let leaf_d = left_leaf_d.min(right_leaf_d).min(center_leaf_d);
            let leaf_alpha = ((1.55 - leaf_d) * 2.2).clamp(0.0, 1.0);
            let crown_alpha = stem_alpha.max(leaf_alpha);

            // Carve a crisp transparent halo around the calyx leaves so they remain
            // distinct against the solid tomato body
            let halo_alpha = ((leaf_d - 2.35) * 2.2).clamp(0.0, 1.0);
            alpha = (alpha * halo_alpha).max(crown_alpha);

            // 3. Punch out the 5 minimalist timer ticks along the bottom (negative space)
            let t_center = dist_to_segment(fx, fy, 18.0, 26.2, 18.0, 29.4) - 0.85;
            let t_mid_l = dist_to_segment(fx, fy, 14.2, 26.5, 14.2, 28.8) - 0.70;
            let t_mid_r = dist_to_segment(fx, fy, 21.8, 26.5, 21.8, 28.8) - 0.70;
            let t_out_l = dist_to_segment(fx, fy, 10.6, 25.8, 11.1, 27.7) - 0.60;
            let t_out_r = dist_to_segment(fx, fy, 25.4, 25.8, 24.9, 27.7) - 0.60;

            let min_tick = t_center.min(t_mid_l).min(t_mid_r).min(t_out_l).min(t_out_r);
            let tick_cutout = (min_tick * 2.2).clamp(0.0, 1.0);
            alpha *= tick_cutout;

            let idx = ((y * size + x) * 4) as usize;
            rgba[idx] = 0;
            rgba[idx + 1] = 0;
            rgba[idx + 2] = 0;
            rgba[idx + 3] = (alpha * 255.0) as u8;
        }
    }

    Icon::from_rgba(rgba, size, size).expect("Failed to create template icon")
}

impl ApplicationHandler<UserEvent> for PomoTrayApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.tray_icon.is_none() {
            let menu = self.build_menu();
            let icon = build_template_tomato_icon();

            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_icon_templated(icon)
                .with_tooltip("Pomo Timer")
                .build()
                .expect("Failed to create macOS status bar item");

            self.tray_icon = Some(tray);
            self.update_tray_display();
        }

        if self.popup.is_none() {
            let mut popup = PopupWindow::new(event_loop);
            let recent = self.db.get_recent_projects(4).unwrap_or_default();
            let today = self.db.get_today_stats().unwrap_or_default();
            popup.update_data(recent, today);
            self.popup = Some(popup);
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let action = {
            let Some(popup) = &mut self.popup else { return };
            if popup.window.id() != id {
                return;
            }

            popup.on_window_event(&event);

            match event {
                WindowEvent::CloseRequested => PopupAction::Close,
                WindowEvent::Focused(false) => {
                    if popup.should_close_on_unfocus() {
                        PopupAction::Close
                    } else {
                        PopupAction::None
                    }
                }
                WindowEvent::RedrawRequested => {
                    if !popup.window.is_visible().unwrap_or(false) {
                        return;
                    }
                    let run_state = match self.state.lock().unwrap().session {
                        None => TimerRunState::Idle,
                        Some(ActiveSession::Running { .. }) => TimerRunState::Running,
                        Some(ActiveSession::Paused { .. }) => TimerRunState::Paused,
                    };
                    popup.render(run_state, &self.current_status)
                }
                _ => PopupAction::None,
            }
        };

        match action {
            PopupAction::Start { project, mins } => {
                self.start_session(&project, mins);
                if let Some(popup) = &self.popup {
                    popup.hide();
                }
            }
            PopupAction::TogglePause => {
                self.toggle_pause();
                if let Some(popup) = &self.popup {
                    popup.window.request_redraw();
                }
            }
            PopupAction::Stop => {
                self.stop_session();
                if let Some(popup) = &self.popup {
                    popup.hide();
                }
            }
            PopupAction::Close => {
                if let Some(popup) = &self.popup {
                    popup.hide();
                }
            }
            PopupAction::None => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Tick {
                detailed_title,
                is_paused,
            } => {
                self.current_status = detailed_title;
                self.is_paused = is_paused;
                self.update_tray_display();

                if let Some(popup) = &self.popup {
                    if popup.window.is_visible().unwrap_or(false) {
                        popup.window.request_redraw();
                    }
                }
            }
            UserEvent::Finished { project, mins } => {
                self.current_status = "Idle".to_string();
                self.is_paused = false;
                self.pause_resume_item.set_text("Pause / Resume");
                self.update_tray_display();

                let _ = self.db.log_session(&project, mins);

                let updated_menu = self.build_menu();
                if let Some(tray) = &self.tray_icon {
                    tray.set_menu(Some(Box::new(updated_menu)));
                }

                #[cfg(target_os = "macos")]
                let _ = notify_rust::set_application("com.yourname.pomo");

                let _ = Notification::new()
                    .summary("Pomodoro Complete!")
                    .body(&format!("Logged {}m session for [{}].", mins, project))
                    .show();
            }
            UserEvent::MenuClicked(id) => {
                if id == self.show_timer_in_bar_item.id() {
                    let show_in_bar = self.show_timer_in_bar_item.is_checked();
                    let _ = self
                        .db
                        .set_bool_setting("show_countdown_in_bar", show_in_bar);
                    self.update_tray_display();
                } else if id == self.custom_window_item.id() {
                    let recent = self.db.get_recent_projects(4).unwrap_or_default();
                    let today = self.db.get_today_stats().unwrap_or_default();

                    let scale = self
                        .popup
                        .as_ref()
                        .map(|p| p.window.scale_factor())
                        .unwrap_or(2.0);

                    let (x, y) = self
                        .tray_icon
                        .as_ref()
                        .and_then(|t| t.rect())
                        .map(|rect| {
                            (
                                rect.position.x / scale,
                                (rect.position.y + rect.size.height as f64) / scale,
                            )
                        })
                        .unwrap_or((800.0, 30.0));

                    if let Some(popup) = &mut self.popup {
                        popup.update_data(recent, today);
                        popup.show_near_menu_bar(x, y);
                    }
                } else if let Some(project) = self.quick_project_items.get(&id).cloned() {
                    self.start_session(&project, 25);
                } else if id == self.pause_resume_item.id() {
                    self.toggle_pause();
                } else if id == self.stop_item.id() {
                    self.stop_session();
                } else if id == self.quit_item.id() {
                    event_loop.exit();
                }
            }
        }
    }
}

impl PomoTrayApp {
    fn update_tray_display(&self) {
        let is_idle = self.current_status == "Idle";

        let dropdown_label = if is_idle {
            "Status: Idle".to_string()
        } else if self.is_paused {
            format!("Paused: {}", self.current_status)
        } else {
            format!("Running: {}", self.current_status)
        };
        self.status_menu_item.set_text(&dropdown_label);

        if let Some(tray) = &self.tray_icon {
            if is_idle {
                tray.set_title(Some(""));
            } else if self.show_timer_in_bar_item.is_checked() {
                let prefix = if self.is_paused { "⏸ " } else { "" };
                tray.set_title(Some(&format!("{}{}", prefix, self.current_status)));
            } else {
                let indicator = if self.is_paused { " ⏸" } else { " •" };
                tray.set_title(Some(indicator));
            }
        }
    }

    fn build_menu(&mut self) -> Menu {
        let menu = Menu::new();
        self.quick_project_items.clear();

        menu.append(&self.status_menu_item).unwrap();
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        menu.append(&self.custom_window_item).unwrap();
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        let mut projects = self.db.get_recent_projects(3).unwrap_or_default();
        if projects.is_empty() {
            projects.push("general".to_string());
        }

        for proj in projects {
            let item = MenuItem::new(format!("Quick Start 25m [{}]", proj), true, None);
            self.quick_project_items.insert(item.id().clone(), proj);
            menu.append(&item).unwrap();
        }

        menu.append(&PredefinedMenuItem::separator()).unwrap();
        menu.append(&self.pause_resume_item).unwrap();
        menu.append(&self.stop_item).unwrap();
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        menu.append(&self.show_timer_in_bar_item).unwrap();
        menu.append(&PredefinedMenuItem::separator()).unwrap();
        menu.append(&self.quit_item).unwrap();

        menu
    }

    fn start_session(&mut self, project: &str, mins: u64) {
        let mut state = self.state.lock().unwrap();
        state.session = Some(ActiveSession::Running {
            end_time: Instant::now() + Duration::from_secs(mins * 60),
            project: project.to_string(),
            total_mins: mins,
        });
        self.current_status = format!("{:02}:00 [{}]", mins, project);
        self.is_paused = false;
        self.pause_resume_item.set_text("Pause Session");
        self.update_tray_display();
    }

    fn toggle_pause(&mut self) {
        let mut state = self.state.lock().unwrap();
        let next_session = match state.session.take() {
            Some(ActiveSession::Running {
                end_time,
                project,
                total_mins,
            }) => {
                let remaining = end_time.saturating_duration_since(Instant::now());
                let secs = remaining.as_secs();
                self.current_status = format!("{:02}:{:02} [{}]", secs / 60, secs % 60, project);
                self.is_paused = true;
                self.pause_resume_item.set_text("Resume Session");
                self.update_tray_display();
                Some(ActiveSession::Paused {
                    remaining,
                    project,
                    total_mins,
                })
            }
            Some(ActiveSession::Paused {
                remaining,
                project,
                total_mins,
            }) => {
                let secs = remaining.as_secs();
                self.current_status = format!("{:02}:{:02} [{}]", secs / 60, secs % 60, project);
                self.is_paused = false;
                self.pause_resume_item.set_text("Pause Session");
                self.update_tray_display();
                Some(ActiveSession::Running {
                    end_time: Instant::now() + remaining,
                    project,
                    total_mins,
                })
            }
            None => None,
        };
        state.session = next_session;
    }

    fn stop_session(&mut self) {
        let mut state = self.state.lock().unwrap();
        state.session = None;
        self.current_status = "Idle".to_string();
        self.is_paused = false;
        self.pause_resume_item.set_text("Pause / Resume");
        self.update_tray_display();
    }
}

pub fn run_tray(db: Database) -> Result<()> {
    let mut builder = EventLoop::<UserEvent>::with_user_event();

    #[cfg(target_os = "macos")]
    builder.with_activation_policy(ActivationPolicy::Accessory);

    let event_loop = builder.build()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let proxy: EventLoopProxy<UserEvent> = event_loop.create_proxy();

    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(Arc::new(move |event: MenuEvent| {
        let _ = menu_proxy.send_event(UserEvent::MenuClicked(event.id));
    })));

    let state = Arc::new(Mutex::new(TimerState { session: None }));
    let bg_state = Arc::clone(&state);

    thread::spawn(move || {
        let mut last_sent_title = String::new();

        loop {
            thread::sleep(Duration::from_millis(500));

            let mut finished_session = None;
            let mut tick_title = None;

            {
                let mut guard = bg_state.lock().unwrap();
                match &guard.session {
                    Some(ActiveSession::Running {
                        end_time,
                        project,
                        total_mins,
                    }) => {
                        let now = Instant::now();
                        if now >= *end_time {
                            finished_session = Some((project.clone(), *total_mins));
                            guard.session = None;
                        } else {
                            let remaining = end_time.duration_since(now).as_secs();
                            let mins = remaining / 60;
                            let secs = remaining % 60;
                            let title = format!("{:02}:{:02} [{}]", mins, secs, project);
                            if title != last_sent_title {
                                last_sent_title = title.clone();
                                tick_title = Some(title);
                            }
                        }
                    }
                    Some(ActiveSession::Paused { .. }) | None => {
                        last_sent_title.clear();
                    }
                }
            }

            if let Some((project, mins)) = finished_session {
                let _ = proxy.send_event(UserEvent::Finished { project, mins });
            } else if let Some(title) = tick_title {
                let _ = proxy.send_event(UserEvent::Tick {
                    detailed_title: title,
                    is_paused: false,
                });
            }
        }
    });

    let show_countdown_in_bar = db.get_bool_setting("show_countdown_in_bar", true);

    let mut app = PomoTrayApp {
        tray_icon: None,
        popup: None,
        status_menu_item: MenuItem::new("Status: Idle", false, None),
        show_timer_in_bar_item: CheckMenuItem::new(
            "Show Countdown in Bar",
            true,
            show_countdown_in_bar,
            None,
        ),
        custom_window_item: MenuItem::new("New Custom Session...", true, None),
        pause_resume_item: MenuItem::new("Pause / Resume", true, None),
        stop_item: MenuItem::new("Stop Session", true, None),
        quit_item: MenuItem::new("Quit Pomo", true, None),
        quick_project_items: HashMap::new(),
        state,
        db,
        current_status: "Idle".to_string(),
        is_paused: false,
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}