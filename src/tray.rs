use crate::db::Database;
use crate::popup::{PopupAction, PopupWindow, TimerRunState};
use anyhow::Result;
use notify_rust::Notification;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

#[cfg(target_os = "macos")]
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};

#[derive(Debug)]
enum UserEvent {
    Tick(String),
    Finished { project: String, mins: u64 },
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
    custom_window_item: MenuItem,
    pause_resume_item: MenuItem,
    stop_item: MenuItem,
    quit_item: MenuItem,
    quick_project_items: HashMap<MenuId, String>,
    state: Arc<Mutex<TimerState>>,
    db: Database,
    current_status: String,
}

impl ApplicationHandler<UserEvent> for PomoTrayApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.tray_icon.is_none() {
            let menu = self.build_menu();

            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_title("🍅 Idle")
                .with_tooltip("Pomo Timer")
                .build()
                .expect("Failed to create macOS status bar item");

            self.tray_icon = Some(tray);
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
            UserEvent::Tick(title) => {
                self.current_status = title.clone();
                if let Some(tray) = &self.tray_icon {
                    tray.set_title(Some(&title));
                }
                if let Some(popup) = &self.popup {
                    if popup.window.is_visible().unwrap_or(false) {
                        popup.window.request_redraw();
                    }
                }
            }
            UserEvent::Finished { project, mins } => {
                self.current_status = "Idle".to_string();
                self.pause_resume_item.set_text("Pause / Resume");
                if let Some(tray) = &self.tray_icon {
                    tray.set_title(Some("🍅 Idle"));
                }
                let _ = self.db.log_session(&project, mins);

                // Refresh the dynamic menu bar project list with the newly logged project
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
                if id == self.custom_window_item.id() {
                    // Refresh SQLite recent projects & today's stats right before showing popup
                    let recent = self.db.get_recent_projects(4).unwrap_or_default();
                    let today = self.db.get_today_stats().unwrap_or_default();

                    let (x, y) = self
                        .tray_icon
                        .as_ref()
                        .and_then(|t| t.rect())
                        .map(|rect| (rect.position.x, rect.position.y + rect.size.height as f64))
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
    /// Builds the dropdown menu dynamically using the top 3 recent projects from SQLite
    fn build_menu(&mut self) -> Menu {
        let menu = Menu::new();
        self.quick_project_items.clear();

        menu.append(&self.custom_window_item).unwrap();
        menu.append(&PredefinedMenuItem::separator()).unwrap();

        let mut projects = self.db.get_recent_projects(3).unwrap_or_default();
        if projects.is_empty() {
            projects.push("rust".to_string());
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
        let title = format!("🍅 {:02}:00 [{}]", mins, project);
        self.current_status = title.clone();
        self.pause_resume_item.set_text("Pause Session");
        if let Some(tray) = &self.tray_icon {
            tray.set_title(Some(&title));
        }
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
                let title = format!("⏸ {:02}:{:02} [{}]", secs / 60, secs % 60, project);
                self.current_status = title.clone();
                self.pause_resume_item.set_text("Resume Session");
                if let Some(tray) = &self.tray_icon {
                    tray.set_title(Some(&title));
                }
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
                let title = format!("🍅 {:02}:{:02} [{}]", secs / 60, secs % 60, project);
                self.current_status = title.clone();
                self.pause_resume_item.set_text("Pause Session");
                if let Some(tray) = &self.tray_icon {
                    tray.set_title(Some(&title));
                }
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
        self.pause_resume_item.set_text("Pause / Resume");
        if let Some(tray) = &self.tray_icon {
            tray.set_title(Some("🍅 Idle"));
        }
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
                            let title = format!("🍅 {:02}:{:02} [{}]", mins, secs, project);
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
                let _ = proxy.send_event(UserEvent::Tick(title));
            }
        }
    });

    let mut app = PomoTrayApp {
        tray_icon: None,
        popup: None,
        custom_window_item: MenuItem::new("New Custom Session...", true, None),
        pause_resume_item: MenuItem::new("Pause / Resume", true, None),
        stop_item: MenuItem::new("Stop Session", true, None),
        quit_item: MenuItem::new("Quit Pomo", true, None),
        quick_project_items: HashMap::new(),
        state,
        db,
        current_status: "Idle".to_string(),
    };

    event_loop.run_app(&mut app)?;
    Ok(())
}