use std::io;

use crossterm::event::{self, Event, KeyCode};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::tmux::{self, Agent};

enum Field {
    Tag,
    AgentSelect,
}

pub struct NewSessionForm {
    tag: String,
    cursor_pos: usize,
    active: Field,
    agent_index: usize,
    pub result: Option<String>,
}

impl NewSessionForm {
    pub fn new() -> Self {
        NewSessionForm {
            tag: String::new(),
            cursor_pos: 0,
            active: Field::Tag,
            agent_index: 0,
            result: None,
        }
    }

    fn selected_agent(&self) -> Agent {
        Agent::all()[self.agent_index]
    }

    fn launch(&mut self) {
        let (name, cwd) = tmux::default_new_session_info();
        let tag = if self.tag.trim().is_empty() {
            None
        } else {
            Some(self.tag.trim())
        };
        match tmux::create_session(&name, &cwd, self.selected_agent(), tag) {
            Ok(name) => self.result = Some(name),
            Err(_) => self.result = Some(String::new()),
        }
    }

    pub fn handle_key(&mut self, event: event::KeyEvent) {
        match event.code {
            KeyCode::Esc => {
                self.result = Some(String::new());
            }
            KeyCode::Enter => {
                match self.active {
                    Field::Tag => {
                        self.active = Field::AgentSelect;
                    }
                    Field::AgentSelect => {
                        self.launch();
                    }
                }
            }
            KeyCode::Left => {
                if matches!(self.active, Field::AgentSelect) {
                    let n = Agent::all().len();
                    self.agent_index = (self.agent_index + n - 1) % n;
                    return;
                }
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                }
            }
            KeyCode::Right => {
                if matches!(self.active, Field::AgentSelect) {
                    self.agent_index = (self.agent_index + 1) % Agent::all().len();
                    return;
                }
                if self.cursor_pos < self.tag.len() {
                    self.cursor_pos += 1;
                }
            }
            KeyCode::Tab | KeyCode::Down => {
                match self.active {
                    Field::Tag => { self.active = Field::AgentSelect; }
                    Field::AgentSelect => {
                        self.active = Field::Tag;
                        self.cursor_pos = self.tag.len();
                    }
                }
            }
            KeyCode::BackTab | KeyCode::Up => {
                match self.active {
                    Field::Tag => { self.active = Field::AgentSelect; }
                    Field::AgentSelect => {
                        self.active = Field::Tag;
                        self.cursor_pos = self.tag.len();
                    }
                }
            }
            KeyCode::Backspace => {
                if matches!(self.active, Field::AgentSelect) { return; }
                let pos = self.cursor_pos;
                if pos > 0 {
                    self.tag.remove(pos - 1);
                    self.cursor_pos = pos - 1;
                }
            }
            KeyCode::Delete => {
                if matches!(self.active, Field::AgentSelect) { return; }
                let pos = self.cursor_pos;
                if pos < self.tag.len() {
                    self.tag.remove(pos);
                }
            }
            KeyCode::Home => { self.cursor_pos = 0; }
            KeyCode::End => { self.cursor_pos = self.tag.len(); }
            KeyCode::Char(c) => {
                if matches!(self.active, Field::AgentSelect) { return; }
                let pos = self.cursor_pos;
                self.tag.insert(pos, c);
                self.cursor_pos = pos + 1;
            }
            _ => {}
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        let tag_active = matches!(self.active, Field::Tag);
        let tag_block = Block::default()
            .borders(Borders::ALL)
            .title(" Tag (optional) ")
            .border_style(Style::default().fg(if tag_active { Color::Cyan } else { Color::DarkGray }));

        let agent_active = matches!(self.active, Field::AgentSelect);
        let agent_block = Block::default()
            .borders(Borders::ALL)
            .title(" Agent ")
            .border_style(Style::default().fg(if agent_active { Color::Cyan } else { Color::DarkGray }));

        let rows = Layout::vertical([
            Constraint::Length(3), // Tag box
            Constraint::Length(3), // Agent box
            Constraint::Length(1), // Hints
            Constraint::Min(0),
        ])
        .split(area);

        let tag_inner = tag_block.inner(rows[0]);
        frame.render_widget(tag_block, rows[0]);
        frame.render_widget(
            Paragraph::new(self.tag.as_str()).style(Style::default().fg(Color::White)),
            tag_inner,
        );

        let agent_inner = agent_block.inner(rows[1]);
        frame.render_widget(agent_block, rows[1]);
        let agent_label = self.selected_agent().label();
        let agent_line = if agent_active {
            Line::from(vec![
                Span::styled("◀ ", Style::default().fg(Color::Cyan)),
                Span::styled(agent_label, Style::default().fg(Color::White)),
                Span::styled(" ▶", Style::default().fg(Color::Cyan)),
            ])
        } else {
            Line::from(Span::styled(agent_label, Style::default().fg(Color::DarkGray)))
        };
        frame.render_widget(Paragraph::new(agent_line), agent_inner);

        let hint = match self.active {
            Field::Tag => Line::from(vec![
                Span::styled(" Enter", Style::default().fg(Color::Cyan)),
                Span::raw(" next  "),
                Span::styled("Tab", Style::default().fg(Color::Cyan)),
                Span::raw(" switch  "),
                Span::styled("Esc", Style::default().fg(Color::Cyan)),
                Span::raw(" cancel"),
            ]),
            Field::AgentSelect => Line::from(vec![
                Span::styled(" ◀▶", Style::default().fg(Color::Cyan)),
                Span::raw(" select  "),
                Span::styled("Enter", Style::default().fg(Color::Cyan)),
                Span::raw(" launch  "),
                Span::styled("Tab", Style::default().fg(Color::Cyan)),
                Span::raw(" switch  "),
                Span::styled("Esc", Style::default().fg(Color::Cyan)),
                Span::raw(" cancel"),
            ]),
        };
        frame.render_widget(Paragraph::new(hint), rows[2]);

        if matches!(self.active, Field::Tag) {
            frame.set_cursor_position((tag_inner.x + self.cursor_pos as u16, tag_inner.y));
        }
    }
}

/// Run the new-session form as a standalone TUI.
pub fn run_new_session_form() -> io::Result<Option<String>> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::prelude::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    let mut form = NewSessionForm::new();

    loop {
        terminal.draw(|f| form.render(f))?;

        if let Some(ref result) = form.result {
            let name = if result.is_empty() { None } else { Some(result.clone()) };

            crossterm::terminal::disable_raw_mode()?;
            crossterm::execute!(
                terminal.backend_mut(),
                crossterm::terminal::LeaveAlternateScreen
            )?;
            terminal.show_cursor()?;

            return Ok(name);
        }

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                form.handle_key(key);
            }
        }
    }
}
