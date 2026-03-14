use std::collections::BTreeMap;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
};

use crate::app::App;
use crate::session::{Session, SessionStatus};

// Layout constants
const ROOMS_PER_PAGE: usize = 4; // fixed 2x2 grid
const CHAR_WIDTH: u16 = 14;
const CHAR_ART_HEIGHT: u16 = 5;
const CHAR_LABEL_LINES: u16 = 4; // session name + branch + status + context bar
const CHAR_HEIGHT: u16 = CHAR_ART_HEIGHT + CHAR_LABEL_LINES;

// ── Character art ────────────────────────────────────────────────────
// Only Working and Input animate. Idle and New are static (calm).

const CHAR_NEW: [&str; 5] = [
    "  .---.  ", " / . . \\ ", "|   .   |", " \\ ___ / ", "  '---'  ",
];

const CHAR_WORKING: [[&str; 5]; 3] = [
    ["  .---.  ", " / ^.^ \\ ", "|  ===  |", " \\_____/ ", "  d   b  "],
    ["  .---.  ", " / >.< \\ ", "|  ~~~  |", " \\_____/ ", "  d   b  "],
    ["  .---.  ", " / ^.^ \\ ", "|> === <|", " \\_____/ ", "  d   b  "],
];

const CHAR_IDLE: [&str; 5] = [
    " .---. Zz", " / -.- \\  ", "|  ~~~  | ", " \\_____/  ", "          ",
];

const CHAR_INPUT: [[&str; 5]; 3] = [
    [" .---. !!", " / O.O \\  ", "|  ___  | ", " \\_____/  ", "  /   \\   "],
    [" .---.  !", " / o.o \\  ", "|/ ___ \\| ", " \\_____/  ", "  /   \\   "],
    [" .---. !!", " / @.@ \\  ", "|  !!!  | ", " \\_____/  ", "  /   \\   "],
];

// ── Room grouping ────────────────────────────────────────────────────

struct Room {
    name: String,
    session_indices: Vec<usize>,
    has_input: bool,
}

fn group_into_rooms(sessions: &[Session]) -> Vec<Room> {
    let mut map: BTreeMap<String, Vec<usize>> = BTreeMap::new();

    for (i, s) in sessions.iter().enumerate() {
        let basename = if s.cwd.is_empty() {
            "unknown".to_string()
        } else {
            std::path::Path::new(&s.cwd)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| s.cwd.clone())
        };
        map.entry(basename).or_default().push(i);
    }

    let mut rooms: Vec<Room> = map
        .into_iter()
        .map(|(name, indices)| {
            let has_input = indices
                .iter()
                .any(|&i| sessions[i].status == SessionStatus::Input);
            Room {
                name,
                session_indices: indices,
                has_input,
            }
        })
        .collect();

    rooms.sort_by(|a, b| {
        b.has_input
            .cmp(&a.has_input)
            .then_with(|| a.name.cmp(&b.name))
    });

    rooms
}

// ── Animation (only Working and Input) ───────────────────────────────

fn animation_frame(status: &SessionStatus, tick: u64) -> usize {
    match status {
        SessionStatus::Working => ((tick / 2) % 3) as usize, // 400ms
        SessionStatus::Input => (tick % 3) as usize,         // 200ms (urgent)
        _ => 0,                                               // static
    }
}

fn session_phase_offset(session_id: &str) -> u64 {
    session_id
        .bytes()
        .fold(0u64, |a, b| a.wrapping_add(b as u64))
        % 7
}

fn character_art(status: &SessionStatus, frame: usize) -> &'static [&'static str; 5] {
    match status {
        SessionStatus::New => &CHAR_NEW,
        SessionStatus::Working => &CHAR_WORKING[frame % 3],
        SessionStatus::Idle => &CHAR_IDLE,
        SessionStatus::Input => &CHAR_INPUT[frame % 3],
    }
}

fn status_color(status: &SessionStatus) -> Color {
    match status {
        SessionStatus::New => Color::Blue,
        SessionStatus::Working => Color::Green,
        SessionStatus::Idle => Color::DarkGray,
        SessionStatus::Input => Color::Yellow,
    }
}

// ── Fatigue overlay ──────────────────────────────────────────────────

fn apply_fatigue_overlay(art: &[&str; 5], ratio: f64) -> [String; 5] {
    let mut lines: [String; 5] = art.each_ref().map(|s| s.to_string());
    if ratio > 0.90 {
        let len = lines[0].len();
        if len >= 2 {
            lines[0].replace_range(len - 2.., ";;");
        }
        let len = lines[1].len();
        if len >= 1 {
            lines[1].replace_range(len - 1.., "'");
        }
    } else if ratio > 0.75 {
        let len = lines[1].len();
        if len >= 1 {
            lines[1].replace_range(len - 1.., "'");
        }
    }
    lines
}

// ── Context bar ──────────────────────────────────────────────────────

fn context_bar(ratio: f64) -> (String, Color) {
    let bar_width = 6usize;
    let filled = (ratio * bar_width as f64).round().min(bar_width as f64) as usize;
    let empty = bar_width - filled;
    let pct = (ratio * 100.0) as u32;
    let bar = format!(
        "{}{} {}%",
        "\u{2588}".repeat(filled),
        "\u{2591}".repeat(empty),
        pct
    );
    let color = if ratio > 0.75 {
        Color::Red
    } else if ratio > 0.40 {
        Color::Yellow
    } else {
        Color::Green
    };
    (bar, color)
}

// ── Public render entry point ────────────────────────────────────────

/// Call before render to resolve pending zoom requests and clamp page.
pub fn resolve_zoom(app: &mut App) {
    let rooms = group_into_rooms(&app.sessions);
    let total_pages = (rooms.len() + ROOMS_PER_PAGE - 1) / ROOMS_PER_PAGE;
    if total_pages > 0 {
        app.view_page = app.view_page.min(total_pages - 1);
    } else {
        app.view_page = 0;
    }

    if let Some(idx) = app.view_zoom_index.take() {
        let page_start = app.view_page * ROOMS_PER_PAGE;
        if let Some(room) = rooms.get(page_start + idx) {
            app.view_zoomed_room = Some(room.name.clone());
        }
    }
}

pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    render_rooms(frame, app, chunks[0]);
    render_footer(frame, app, chunks[1]);
}

fn render_rooms(frame: &mut Frame, app: &App, area: Rect) {
    let rooms = group_into_rooms(&app.sessions);

    if rooms.is_empty() {
        render_empty(frame, area, app.tick);
        return;
    }

    // Zoomed into a single room?
    if let Some(ref zoomed_name) = app.view_zoomed_room {
        if let Some(room) = rooms.iter().find(|r| &r.name == zoomed_name) {
            render_room(frame, app, room, area, None);
            return;
        }
        // Room disappeared — fall through to grid
    }

    // Paginate: 4 rooms per page
    let total_pages = (rooms.len() + ROOMS_PER_PAGE - 1) / ROOMS_PER_PAGE;
    let page = app.view_page.min(total_pages.saturating_sub(1));
    let page_start = page * ROOMS_PER_PAGE;
    let page_rooms: Vec<&Room> = rooms
        .iter()
        .skip(page_start)
        .take(ROOMS_PER_PAGE)
        .collect();

    // Fixed 2x2 grid
    let v_chunks = Layout::vertical([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(area);

    let top_h = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(v_chunks[0]);
    let bot_h = Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
        .split(v_chunks[1]);

    let cells = [top_h[0], top_h[1], bot_h[0], bot_h[1]];

    for (i, cell) in cells.iter().enumerate() {
        if let Some(room) = page_rooms.get(i) {
            let display_idx = i + 1; // 1-based for key hint
            render_room(frame, app, room, *cell, Some(display_idx));
        } else {
            // Empty cell — render placeholder
            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(30, 30, 30)));
            frame.render_widget(block, *cell);
        }
    }
}

fn render_room(frame: &mut Frame, app: &App, room: &Room, area: Rect, slot_num: Option<usize>) {
    let border_color = if room.has_input {
        if app.tick % 2 == 0 {
            Color::Yellow
        } else {
            Color::White
        }
    } else {
        Color::DarkGray
    };

    let title = match slot_num {
        Some(n) => format!(" [{}] {} ({}) ", n, room.name, room.session_indices.len()),
        None => format!(" {} ({}) ", room.name, room.session_indices.len()),
    };
    let title_style = if room.has_input {
        Style::default().fg(border_color)
    } else {
        Style::default().fg(Color::White)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(title, title_style))
        .padding(Padding::horizontal(1));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let chars_per_row = (inner.width / CHAR_WIDTH).max(1) as usize;
    let char_rows: Vec<&[usize]> = room.session_indices.chunks(chars_per_row).collect();

    // Vertically center characters within the room
    let needed_height = char_rows.len() as u16 * CHAR_HEIGHT;
    let v_pad = inner.height.saturating_sub(needed_height) / 2;
    let char_area = Rect {
        x: inner.x,
        y: inner.y + v_pad,
        width: inner.width,
        height: inner.height.saturating_sub(v_pad),
    };

    let row_constraints: Vec<Constraint> = char_rows
        .iter()
        .map(|_| Constraint::Length(CHAR_HEIGHT))
        .collect();
    let v_chunks = Layout::vertical(row_constraints).split(char_area);

    for (row_idx, indices) in char_rows.iter().enumerate() {
        if row_idx >= v_chunks.len() {
            break;
        }
        let col_constraints: Vec<Constraint> = indices
            .iter()
            .map(|_| Constraint::Length(CHAR_WIDTH))
            .collect();
        let h_chunks = Layout::horizontal(col_constraints).split(v_chunks[row_idx]);

        for (col_idx, &session_idx) in indices.iter().enumerate() {
            if col_idx >= h_chunks.len() {
                break;
            }
            render_character(frame, &app.sessions[session_idx], h_chunks[col_idx], app.tick);
        }
    }
}

fn render_character(frame: &mut Frame, session: &Session, area: Rect, tick: u64) {
    if area.height < 3 || area.width < 4 {
        return;
    }

    let offset = session_phase_offset(&session.session_id);
    let anim_frame = animation_frame(&session.status, tick + offset);
    let art = character_art(&session.status, anim_frame);
    let ratio = session.token_ratio();

    let art_lines = if ratio > 0.75 {
        apply_fatigue_overlay(art, ratio)
    } else {
        art.each_ref().map(|s| s.to_string())
    };

    // Pulse color for Input status
    let color = if session.status == SessionStatus::Input {
        if tick % 2 == 0 {
            Color::Yellow
        } else {
            Color::White
        }
    } else {
        status_color(&session.status)
    };

    let mut lines: Vec<Line> = Vec::new();

    // Character art (5 lines)
    for line in &art_lines {
        let truncated = truncate_str(line, area.width as usize);
        lines.push(Line::from(Span::styled(
            truncated,
            Style::default().fg(color),
        )));
    }

    // Session name
    let name = session.tmux_session.as_deref().unwrap_or("???");
    lines.push(Line::from(Span::styled(
        truncate_str(name, area.width as usize),
        Style::default().fg(Color::White),
    )));

    // Git branch
    let branch = session.branch.as_deref().unwrap_or("");
    lines.push(Line::from(Span::styled(
        truncate_str(branch, area.width as usize),
        Style::default().fg(Color::Green),
    )));

    // Status label
    lines.push(Line::from(Span::styled(
        session.status.label(),
        Style::default().fg(color),
    )));

    // Context bar
    let (bar_str, bar_color) = context_bar(ratio);
    lines.push(Line::from(Span::styled(
        truncate_str(&bar_str, area.width as usize),
        Style::default().fg(bar_color),
    )));

    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

fn render_empty(frame: &mut Frame, area: Rect, _tick: u64) {
    let art = &CHAR_IDLE;
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));
    for &line in art {
        lines.push(Line::from(Span::styled(
            line,
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "No active sessions",
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let rooms = group_into_rooms(&app.sessions);
    let total_pages = (rooms.len() + ROOMS_PER_PAGE - 1) / ROOMS_PER_PAGE;
    let page = app.view_page.min(total_pages.saturating_sub(1));

    let mut spans = vec![];

    if app.view_zoomed_room.is_some() {
        spans.push(Span::styled("Esc", Style::default().fg(Color::Cyan)));
        spans.push(Span::raw(" back  "));
    } else {
        spans.push(Span::styled("1-4", Style::default().fg(Color::Cyan)));
        spans.push(Span::raw(" zoom  "));
        if total_pages > 1 {
            spans.push(Span::styled("h/l", Style::default().fg(Color::Cyan)));
            spans.push(Span::raw(format!(" page ({}/{})  ", page + 1, total_pages)));
        }
    }

    spans.push(Span::styled("v", Style::default().fg(Color::Cyan)));
    spans.push(Span::raw(" table  "));
    spans.push(Span::styled("r", Style::default().fg(Color::Cyan)));
    spans.push(Span::raw(" refresh  "));
    spans.push(Span::styled("q", Style::default().fg(Color::Cyan)));
    spans.push(Span::raw(" quit"));

    let footer = Paragraph::new(Line::from(spans));
    frame.render_widget(footer, area);
}

// ── Helpers ──────────────────────────────────────────────────────────

fn truncate_str(s: &str, max_width: usize) -> String {
    let char_count: usize = s.chars().count();
    if char_count <= max_width {
        s.to_string()
    } else if max_width > 1 {
        let truncated: String = s.chars().take(max_width - 1).collect();
        format!("{}\u{2026}", truncated)
    } else {
        String::new()
    }
}
