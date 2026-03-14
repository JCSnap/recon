use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent};

use crate::session::{self, Session};
use crate::tmux;

#[derive(Clone, Copy, PartialEq)]
pub enum ViewMode {
    Table,
    View,
}

pub struct App {
    pub sessions: Vec<Session>,
    pub selected: usize,
    pub effort_level: String,
    pub should_quit: bool,
    pub view_mode: ViewMode,
    pub tick: u64,
    pub view_page: usize,
    pub view_zoomed_room: Option<String>, // room name when zoomed in
    pub view_zoom_index: Option<usize>,  // pending zoom request from key press
    prev_sessions: HashMap<String, Session>,
}

impl App {
    pub fn new() -> Self {
        let effort_level = read_effort_level().unwrap_or_else(|| "medium".to_string());
        App {
            sessions: Vec::new(),
            selected: 0,
            effort_level,
            should_quit: false,
            view_mode: ViewMode::Table,
            tick: 0,
            view_page: 0,
            view_zoomed_room: None,
            view_zoom_index: None,
            prev_sessions: HashMap::new(),
        }
    }

    pub fn refresh(&mut self) {
        let sessions: Vec<Session> = session::discover_sessions(&self.prev_sessions)
            .into_iter()
            .filter(|s| s.tmux_session.is_some())
            .collect();

        self.prev_sessions = sessions
            .iter()
            .map(|s| (s.session_id.clone(), s.clone()))
            .collect();

        self.sessions = sessions;

        if self.selected >= self.sessions.len() && !self.sessions.is_empty() {
            self.selected = self.sessions.len() - 1;
        }
    }

    pub fn advance_tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.view_mode {
            ViewMode::Table => self.handle_key_table(key),
            ViewMode::View => self.handle_key_view(key),
        }
    }

    fn handle_key_table(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('v') => self.view_mode = ViewMode::View,
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.sessions.is_empty() {
                    self.selected = (self.selected + 1).min(self.sessions.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            KeyCode::Enter => {
                if let Some(session) = self.sessions.get(self.selected) {
                    if let Some(name) = &session.tmux_session {
                        tmux::switch_to_session(name);
                        self.should_quit = true;
                    }
                }
            }
            KeyCode::Char('x') => {
                if let Some(session) = self.sessions.get(self.selected) {
                    if let Some(name) = &session.tmux_session {
                        tmux::kill_session(name);
                        self.refresh();
                    }
                }
            }
            KeyCode::Char('r') => {
                self.refresh();
            }
            _ => {}
        }
    }

    fn handle_key_view(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Esc => {
                if self.view_zoomed_room.is_some() {
                    self.view_zoomed_room = None; // zoom out
                } else {
                    self.should_quit = true;
                }
            }
            KeyCode::Char('v') => {
                self.view_zoomed_room = None;
                self.view_mode = ViewMode::Table;
            }
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('l') | KeyCode::Right => {
                self.view_page = self.view_page.saturating_add(1);
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.view_page = self.view_page.saturating_sub(1);
            }
            // 1-9 to zoom into room by index on current page
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as usize) - ('1' as usize);
                // Room name will be resolved by view_ui via view_zoomed_room
                // Store the index temporarily; view_ui will set the actual room name
                self.view_zoom_index = Some(idx);
            }
            _ => {}
        }
    }

    pub fn to_json(&self) -> String {
        let sessions: Vec<serde_json::Value> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                serde_json::json!({
                    "index": i + 1,
                    "session_id": s.session_id,
                    "project_name": s.project_name,
                    "branch": s.branch,
                    "cwd": s.cwd,
                    "tmux_session": s.tmux_session,
                    "model": s.model,
                    "model_display": s.model_display(&self.effort_level),
                    "total_input_tokens": s.total_input_tokens,
                    "total_output_tokens": s.total_output_tokens,
                    "context_display": s.token_display(),
                    "token_ratio": s.token_ratio(),
                    "status": s.status.label(),
                    "pid": s.pid,
                    "last_activity": s.last_activity,
                    "started_at": s.started_at,
                })
            })
            .collect();

        serde_json::to_string_pretty(&serde_json::json!({
            "sessions": sessions,
            "effort_level": self.effort_level,
        }))
        .unwrap_or_else(|_| "{}".to_string())
    }
}

fn read_effort_level() -> Option<String> {
    let home = dirs::home_dir()?;
    let path = home.join(".claude").join("settings.json");
    let content = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&content).ok()?;
    v.get("effortLevel")?.as_str().map(|s| s.to_string())
}
