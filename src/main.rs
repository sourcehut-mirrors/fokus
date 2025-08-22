//  A minimalist terminal‐based focus timer and stopwatch with daily logging, built in Rust
//  Copyright (C) 2025  Arda Yılmaz
//
//  This program is free software: you can redistribute it and/or modify
//  it under the terms of the GNU General Public License as published by
//  the Free Software Foundation, either version 3 of the License, or
//  (at your option) any later version.
//
//  This program is distributed in the hope that it will be useful,
//  but WITHOUT ANY WARRANTY; without even the implied warranty of
//  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//  GNU General Public License for more details.
//
//  You should have received a copy of the GNU General Public License
//  along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{
    collections::HashMap,
    fs,
    io::{self, stdout},
    path::PathBuf,
    time::{Duration, Instant},
};
use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json;

const CONFIG_TIMER_MIN: u64 = 1;
const CONFIG_TIMER_MAX: u64 = 999;

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    default_timer_duration: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_timer_duration: 25,
        }
    }
}

impl Config {
    fn config_path() -> Option<PathBuf> {
        dirs_next::config_dir().map(|d| d.join("fokus").join("config.toml"))
    }

    fn load_or_create() -> io::Result<Config> {
        let cfg = match Self::config_path() {
            Some(path) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?; 
                }

                if path.exists() {
                    let s = fs::read_to_string(&path)?;
                    match toml::from_str::<Config>(&s) {
                        Ok(cfg) => {

                            if cfg.default_timer_duration < CONFIG_TIMER_MIN || cfg.default_timer_duration > CONFIG_TIMER_MAX {
                                let def = Config::default();
                                let mut toml_s = toml::to_string_pretty(&def).unwrap();
                                toml_s = format!(
                                    "# fokus Configuration File\n\n\
                                 # Default timer duration (in minutes)\n\
                                 # Must be between {} and {}\n{}",
                                 CONFIG_TIMER_MIN, CONFIG_TIMER_MAX, toml_s
                                );
                                fs::write(&path, toml_s)?;
                                def
                            } else {
                                cfg
                            }
                        }
                        Err(_) => {
                            let def = Config::default();
                            let mut toml_s = toml::to_string_pretty(&def).unwrap();
                            toml_s = format!("# fokus Configuration File\n\n# Default timer duration (in minutes)\n# Must be between {} and {}\n{}", CONFIG_TIMER_MIN, CONFIG_TIMER_MAX, toml_s);
                            fs::write(&path, toml_s)?;
                            def
                        }
                    }
                } else {

                    let def = Config::default();
                    let mut toml_s = toml::to_string_pretty(&def).unwrap();
                    toml_s = format!("# fokus Configuration File\n\n# Default timer duration (in minutes)\n# Must be between {} and {}\n{}", CONFIG_TIMER_MIN, CONFIG_TIMER_MAX, toml_s);
                    fs::write(&path, toml_s)?;
                    def
                }
            }
            None => {

                Config::default()
            }
        };
        Ok(cfg)
    }
}

fn history_path() -> Option<PathBuf> {
    dirs_next::config_dir().map(|d| d.join("fokus").join("history.json"))
}

fn load_or_create_history() -> io::Result<HashMap<String, u64>> {
    if let Some(path) = history_path() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?; 
        }

        if path.exists() {
            let s = fs::read_to_string(&path)?;
            match serde_json::from_str::<HashMap<String, u64>>(&s) {
                Ok(map) => Ok(map),
                Err(_) => {

                    let date_str = Local::now().format("%Y%m%d_%H%M%S").to_string();
                    let backup_path = path.with_file_name(format!(
                            "history_{}.json.bak",
                            date_str
                    ));

                    fs::copy(&path, &backup_path)?;

                    let empty: HashMap<String, u64> = HashMap::new();
                    let s2 = serde_json::to_string_pretty(&empty)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    fs::write(&path, s2)?;
                    Ok(empty)
                }
            }
        } else {

            let empty: HashMap<String, u64> = HashMap::new();
            let s = serde_json::to_string_pretty(&empty)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            fs::write(&path, s)?;
            Ok(empty)
        }
    } else {
        Ok(HashMap::new())
    }
}

fn _save_history(map: &HashMap<String, u64>) -> io::Result<()> {
    if let Some(path) = history_path() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let s = serde_json::to_string_pretty(map)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(path, s)?;
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "Config directory not found"))
    }
}

fn lock_path() -> Option<PathBuf> {
    dirs_next::config_dir().map(|d| d.join("fokus").join("fokus.lock"))
}

fn acquire_lock() -> io::Result<(fs::File, PathBuf)> {
    if let Some(path) = lock_path() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        if path.exists() {
            if let Ok(s) = fs::read_to_string(&path) {
                if let Ok(pid) = s.trim().parse::<u32>() {
                    let proc_path = PathBuf::from("/proc").join(pid.to_string());
                    if proc_path.exists() {
                        return Err(io::Error::new(
                                io::ErrorKind::AlreadyExists,
                                "Another instance is already running. There can only be one fokus instance at a time. Please kill the other instance before starting a new one.",
                        ));
                    } else {
                        let _ = fs::remove_file(&path);
                    }
                } else {
                    let _ = fs::remove_file(&path);
                }
            } else {
                let _ = fs::remove_file(&path);
            }
        }

        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let pid = std::process::id();
        use std::io::Write;
        let _ = write!(f, "{}", pid);
        Ok((f, path))
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "Config directory could not be found."))
    }
}

fn main() -> io::Result<()> {

    let config = Config::load_or_create()?;

    let (lock_file, lock_path_buf) = match acquire_lock() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("There is something wrong: {}", e);
            std::process::exit(1);
        }
    };

    let mut timer_total = Duration::from_secs(config.default_timer_duration * 60);

    let mut history = load_or_create_history()?;

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?; 
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let header_pages = ["< Page 1 of 3 >", "< Page 2 of 3 >", "< Page 3 of 3 >"];
    let mut header_page_index = 0;

    let mut stopwatch_start = Instant::now();
    let mut stopwatch_running = false;
    let mut stopwatch_display = "00:00.00".to_string();

    let extra = Duration::from_secs(60); 
    let timer_max = Duration::from_secs(60*999);
    let timer_min = Duration::from_secs(60*1);

    let mut timer_start = Instant::now();
    let mut timer_running = false;
    let mut timer_display = format_duration(timer_total);
    let mut timer_logged = false; 
    let mut timer_done = false; 

    let mut history_offset = 0; 

    terminal.clear()?;
    terminal.hide_cursor()?;

    loop {

        let area = terminal.size()?;
        let middle_height = area.height / 2; 
        let visible_height = (middle_height as usize).saturating_sub(2);

        terminal.draw(|f| {

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),      
                    Constraint::Min(0),         
                    Constraint::Length(2),      
                ])
                .split(f.area());

            let header = Paragraph::new(format!("\n{}", header_pages[header_page_index]))
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Cyan));
            f.render_widget(header, chunks[0]);

            let middle_chunks = if header_page_index == 2 {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Percentage(25), 
                        Constraint::Min(visible_height as u16), 
                        Constraint::Length(1),      
                        Constraint::Percentage(25), 
                    ])
                    .split(chunks[1])
            } else {

                let content_height: usize = 3;
                let area_h = chunks[1].height as usize;

                let remaining = area_h.saturating_sub(content_height + 1);
                let top = (remaining / 2) as u16;
                let bottom = (remaining - (remaining / 2)) as u16;

                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(top),                    
                        Constraint::Length(content_height as u16),  
                        Constraint::Length(1),                      
                        Constraint::Length(bottom),                 
                    ])
                    .split(chunks[1])
            };

            let middle_inner = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(30), 
                    Constraint::Percentage(40), 
                    Constraint::Percentage(30), 
                ])
                .split(middle_chunks[1]);

            let middle_text = match header_page_index {
                0 => {

                    if stopwatch_running {
                        let elapsed = stopwatch_start.elapsed();
                        stopwatch_display = format!(
                            "{:02}:{:02}.{:02}",
                            elapsed.as_secs() / 60,
                            elapsed.as_secs() % 60,
                            elapsed.subsec_millis() / 10
                        );
                    }
                    stopwatch_display.clone()
                }
                1 => {
                    if timer_running {
                        let elapsed = timer_start.elapsed();
                        let remaining = if elapsed >= timer_total {
                            Duration::ZERO
                        } else {
                            timer_total - elapsed
                        };
                        timer_display = format_duration(remaining);
                        if remaining == Duration::ZERO && !timer_logged {
                            timer_running = false;
                            timer_done = true;
                            let minutes = timer_total.as_secs() / 60;
                            if minutes > 0 {
                                let today = Local::now().format("%Y-%m-%d").to_string();
                                *history.entry(today).or_insert(0) += minutes;
                                if let Err(e) = _save_history(&history) {
                                    eprintln!("Failed to save history: {}", e);
                                }
                            }
                            timer_logged = true;
                        }
                    }
                    timer_display.clone()
                }
                2 => {

                    let mut table = String::new();

                    let mut parsed: Vec<(chrono::NaiveDate, String)> = Vec::new();
                    let mut unparsable: Vec<String> = Vec::new();
                    for k in history.keys() {
                        match NaiveDate::parse_from_str(k, "%Y-%m-%d") {
                            Ok(d) => parsed.push((d, k.clone())),
                            Err(_) => unparsable.push(k.clone()),
                        }
                    }
                    parsed.sort_by(|a, b| b.0.cmp(&a.0));

                    parsed.extend(unparsable.into_iter().map(|s| (NaiveDate::from_ymd_opt(1970,1,1).unwrap(), s)));

                    let widget_height = middle_inner[1].height as usize;
                    let header_rows = 2; 
                    let available_rows = widget_height.saturating_sub(header_rows);

                    let total_rows = parsed.len();

                    if total_rows <= available_rows {
                        history_offset = 0;
                    } else if history_offset > total_rows.saturating_sub(available_rows) {
                        history_offset = total_rows.saturating_sub(available_rows);
                    }

                    let end = (history_offset + available_rows).min(total_rows);
                    let visible = &parsed[history_offset..end];

                    table.push_str(&format!("{:<11} | {:>6}\n", "Date", "Minutes"));
                    table.push_str(&"-".repeat(21));
                    table.push('\n');

                    for (_d, key) in visible {
                        let minutes = history.get(key.as_str()).copied().unwrap_or(0);
                        table.push_str(&format!("{:<11} | {:>6}\n", key, minutes));
                    }

                    table
                }
                _ => "".to_string(),
            };

            let middle_style = if header_page_index == 1 && !timer_running && timer_display == "00:00.00" {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::default())
            };

            let middle = Paragraph::new(middle_text)
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                    .borders(Borders::ALL) 
                    .border_style(Style::default().fg(Color::Gray)) 
                    .title(match header_page_index {
                        0 => " Stopwatch ",
                        1 => " Timer ",
                        2 => " History ",
                        _ => "",
                    })
                    .title_style(Style::default().fg(Color::Green)) 
                    .title_alignment(Alignment::Left),
                )
                .style(middle_style);
            f.render_widget(middle, middle_inner[1]);

            let today = Local::now().format("%Y-%m-%d").to_string();
            let minutes_today = history.get(&today).cloned().unwrap_or(0);
            let focused_minutes_text = Paragraph::new(format!("{} minutes focused today", minutes_today)) 
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Yellow));
            if header_page_index != 2 && !( (header_page_index == 0 && stopwatch_running) || (header_page_index == 1 && timer_running) ) {        
                f.render_widget(focused_minutes_text, middle_chunks[2]);
            }

            let footer = Paragraph::new("[space] Start/Reset [q] Quit [h]/[l] Change Page [j]/[k] Adjust/Scroll")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Gray));
            f.render_widget(footer, chunks[2]);
        })?;

        if event::poll(std::time::Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Right | KeyCode::Char('l') => {
                        if !timer_running && !stopwatch_running {
                            header_page_index = (header_page_index + 1) % header_pages.len();
                        }
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        if !timer_running && !stopwatch_running {
                            header_page_index = (header_page_index + header_pages.len() - 1) % header_pages.len();
                        }    
                    }

                    KeyCode::Up | KeyCode::Char('k') => {
                        match header_page_index {
                            1 => { 
                                if !timer_running {
                                    timer_total = (timer_total + extra).min(timer_max);
                                    timer_display = format_duration(timer_total);
                                }
                            },
                            2 => { 
                                if history_offset > 0 {
                                    history_offset -= 1;
                                }
                            },
                            _ => {}
                        }
                    },
                    KeyCode::Down | KeyCode::Char('j') => {
                        match header_page_index {
                            1 => { 
                                if !timer_running {
                                    timer_total = (timer_total.saturating_sub(extra)).max(timer_min);
                                    timer_display = format_duration(timer_total);
                                }

                            },

                            2 => { 
                                history_offset = history_offset.saturating_add(1);
                            },
                            _ => {}
                        }
                    },
                    KeyCode::Char(' ') => match header_page_index {
                        0 => {

                            if stopwatch_running {
                                stopwatch_running = false;

                                let elapsed = stopwatch_start.elapsed();
                                let minutes = (elapsed.as_secs() / 60) as u64;
                                if minutes > 0 {
                                    let today = Local::now().format("%Y-%m-%d").to_string();
                                    *history.entry(today).or_insert(0) += minutes;
                                    if let Err(e) = _save_history(&history) {
            eprintln!("Failed to save history: {}", e);
        }
                                }
                                stopwatch_display = "00:00.00".to_string();
                            } else {
                                stopwatch_running = true;
                                stopwatch_start = Instant::now();
                            }
                        }
                        1 => {

                            if timer_done {

                                timer_display = format_duration(timer_total);
                                timer_done = false;
                            } else if timer_running {
                                timer_running = false;
                                timer_display = format_duration(timer_total);
                            } else {
                                timer_running = true;
                                timer_start = Instant::now();
                                timer_logged = false; 
                            }
                        }
                        _ => {}
                    },
                    KeyCode::Char('q') => {
                        if stopwatch_running {
                            let elapsed = stopwatch_start.elapsed();
                            let minutes = (elapsed.as_secs() / 60) as u64;
                            if minutes > 0 {
                                let today = Local::now().format("%Y-%m-%d").to_string();
                                *history.entry(today).or_insert(0) += minutes;
                                if let Err(e) = _save_history(&history) {
            eprintln!("Failed to save history: {}", e);
        }
                            }
                        }
                        break
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = _save_history(&history) {
        eprintln!("Failed to save history: {}", e);
    }

    let _ = fs::remove_file(lock_path_buf);
    drop(lock_file);
    Ok(())
}

fn format_duration(dur: Duration) -> String {
    let mins = dur.as_secs() / 60;
    let secs = dur.as_secs() % 60;
    let centis = dur.subsec_millis() / 10;
    format!("{:02}:{:02}.{:02}", mins, secs, centis)
}
