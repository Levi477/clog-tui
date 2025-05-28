use chrono::Local;
use directories::ProjectDirs;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use serde_json::Value;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::Command;
use tempfile::NamedTempFile;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

use clog_rs::*;

#[derive(Clone)]
enum AppState {
    SelectUser,
    EnterNewUser,
    EnterPassword(String),
    SelectFolder(String, String),
    SelectFile(String, String, String),
    EditOrViewFile(String, String, String, String),
    InputPrompt(String, Box<AppState>), // prompt text, next state
    AddPagePrompt(String, String, String), // user_path, password, folder
    Done,
}

struct App {
    state: AppState,
    selected_index: usize,
    input_buffer: String,
    message: String,
    show_help: bool,
    data_dir: PathBuf,
}

impl App {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let project_dirs =
            ProjectDirs::from("com", "levi", "clog").ok_or("Failed to get project directories")?;

        let data_dir = project_dirs.data_dir().to_path_buf();

        // Create data directory if it doesn't exist
        fs::create_dir_all(&data_dir)?;

        Ok(Self {
            state: AppState::SelectUser,
            selected_index: 0,
            input_buffer: String::new(),
            message: String::new(),
            show_help: false,
            data_dir,
        })
    }

    fn reset_selection(&mut self) {
        self.selected_index = 0;
    }

    fn get_help_text(&self) -> &'static str {
        match &self.state {
            AppState::SelectUser => "↑/↓ or j/k: Navigate | Enter: Select | q: Quit",
            AppState::EnterNewUser => {
                "Enter username and password when prompted | Esc: Back | q: Quit"
            }
            AppState::EnterPassword(_) => "Enter password when prompted | Esc: Back | q: Quit",
            AppState::SelectFolder(_, _) => {
                "↑/↓ or j/k: Navigate | Enter: Select | b/Esc: Back | q: Quit"
            }
            AppState::SelectFile(_, _, _) => {
                "↑/↓ or j/k: Navigate | Enter: Select | b/Esc: Back | q: Quit"
            }
            AppState::EditOrViewFile(_, _, _, _) => "Page will open in editor | q: Quit",
            AppState::InputPrompt(_, _) => "Type your input | Enter: Confirm | Esc: Cancel",
            AppState::AddPagePrompt(_, _, _) => "Type page name | Enter: Confirm | Esc: Cancel",
            AppState::Done => "Press any key to exit",
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new()?;

    loop {
        // Clear the terminal to prevent message persistence
        terminal.clear()?;

        match app.state.clone() {
            AppState::SelectUser => {
                let user_files = list_clog_files(&app.data_dir);
                let mut display_items = Vec::new();

                // Add user files with creation dates
                for file in &user_files {
                    let file_path = app.data_dir.join(file);
                    let creation_date = get_user_creation_date(&file_path).unwrap_or_default();
                    display_items.push((file.clone(), creation_date));
                }

                display_items.push(("Add New User".to_string(), String::new()));
                let help_text = app.get_help_text();

                if let Some(selection) = select_menu_with_metadata(
                    &mut terminal,
                    "Select User",
                    &display_items,
                    &mut app.selected_index,
                    &mut app.show_help,
                    help_text,
                )? {
                    if selection == "Add New User" {
                        app.input_buffer.clear();
                        app.state = AppState::InputPrompt(
                            "Enter new username:".to_string(),
                            Box::new(AppState::EnterNewUser),
                        );
                    } else {
                        app.input_buffer = selection.clone();
                        app.state = AppState::InputPrompt(
                            "Enter password:".to_string(),
                            Box::new(AppState::EnterPassword(selection)),
                        );
                    }
                    app.reset_selection();
                }
            }
            AppState::EnterNewUser => {
                // This state is handled by InputPrompt now
                unreachable!();
            }
            AppState::EnterPassword(user_path) => {
                // This state is handled by InputPrompt now
                unreachable!();
            }
            AppState::InputPrompt(prompt, next_state) => {
                let help_text = app.get_help_text();
                if let Some(input) = prompt_input_in_app(
                    &mut terminal,
                    &prompt,
                    &mut app.input_buffer,
                    &mut app.show_help,
                    help_text,
                )? {
                    match *next_state {
                        AppState::EnterNewUser => {
                            let username = input;
                            app.state = AppState::InputPrompt(
                                "Enter password:".to_string(),
                                Box::new(AppState::SelectFolder(
                                    format!("{}.clog", username),
                                    String::new(),
                                )),
                            );
                        }
                        AppState::EnterPassword(user_path) => {
                            let password = input;
                            let file_path = app.data_dir.join(&user_path);
                            // Test password by trying to get metadata
                            match std::panic::catch_unwind(|| {
                                get_json_metadata(&password, file_path.to_str().unwrap())
                            }) {
                                Ok(_) => {
                                    app.state = AppState::SelectFolder(user_path, password);
                                    app.reset_selection();
                                }
                                Err(_) => {
                                    show_message(&mut terminal, "Incorrect password!", "Error")?;
                                    app.state = AppState::SelectUser;
                                    app.reset_selection();
                                }
                            }
                        }
                        AppState::SelectFolder(user_path, password) => {
                            // This is for creating new user
                            let username = user_path.trim_end_matches(".clog");
                            let file_path = app.data_dir.join(&user_path);
                            add_new_user(&input, file_path.to_str().unwrap());
                            app.state = AppState::SelectFolder(user_path, input);
                            app.reset_selection();
                        }
                        _ => {}
                    }
                    app.input_buffer.clear();
                } else {
                    // User cancelled, go back
                    app.state = AppState::SelectUser;
                    app.reset_selection();
                    app.input_buffer.clear();
                }
            }
            AppState::AddPagePrompt(user_path, password, folder) => {
                let help_text = app.get_help_text();
                if let Some(filename) = prompt_input_in_app(
                    &mut terminal,
                    "Enter page name:",
                    &mut app.input_buffer,
                    &mut app.show_help,
                    help_text,
                )? {
                    // Create empty file first, then open in editor
                    let initial_content = "";
                    match edit_file_with_editor(initial_content) {
                        Ok(content) => {
                            if !content.trim().is_empty() {
                                let file_path = app.data_dir.join(&user_path);
                                add_file(
                                    &password,
                                    file_path.to_str().unwrap(),
                                    &filename,
                                    &content,
                                );
                                show_message(
                                    &mut terminal,
                                    &format!("Page '{}' added successfully!", filename),
                                    "Success",
                                )?;
                            } else {
                                show_message(
                                    &mut terminal,
                                    "Page creation cancelled (empty content)",
                                    "Info",
                                )?;
                            }
                        }
                        Err(e) => {
                            show_message(
                                &mut terminal,
                                &format!("Error creating page: {}", e),
                                "Error",
                            )?;
                        }
                    }
                    app.state = AppState::SelectFile(user_path, password, folder);
                    app.reset_selection();
                    app.input_buffer.clear();
                } else {
                    // User cancelled
                    app.state = AppState::SelectFile(user_path, password, folder);
                    app.reset_selection();
                    app.input_buffer.clear();
                }
            }
            AppState::SelectFolder(user_path, password) => {
                let file_path = app.data_dir.join(&user_path);
                let metadata_str = get_json_metadata(&password, file_path.to_str().unwrap());
                let metadata: Value = match serde_json::from_str(&metadata_str) {
                    Ok(m) => m,
                    Err(_) => {
                        show_message(&mut terminal, "Error parsing metadata", "Error")?;
                        app.state = AppState::SelectUser;
                        app.reset_selection();
                        continue;
                    }
                };

                let mut folders: Vec<String> = metadata["folders"]
                    .as_object()
                    .map(|obj| obj.keys().cloned().collect())
                    .unwrap_or_default();

                folders.sort();

                // Convert to display format
                let display_items: Vec<(String, String)> = folders
                    .into_iter()
                    .map(|folder| (folder, String::new()))
                    .collect();

                let help_text = app.get_help_text();

                if let Some(NavigationResult::Selected(folder)) =
                    select_menu_with_back_and_metadata(
                        &mut terminal,
                        "Select Chapter",
                        &display_items,
                        &mut app.selected_index,
                        &mut app.show_help,
                        help_text,
                    )?
                {
                    app.state = AppState::SelectFile(user_path, password, folder);
                    app.reset_selection();
                } else {
                    app.state = AppState::SelectUser;
                    app.reset_selection();
                }
            }
            AppState::SelectFile(user_path, password, folder) => {
                let file_path = app.data_dir.join(&user_path);
                let metadata_str = get_json_metadata(&password, file_path.to_str().unwrap());
                let metadata: Value = match serde_json::from_str(&metadata_str) {
                    Ok(m) => m,
                    Err(_) => {
                        show_message(&mut terminal, "Error parsing metadata", "Error")?;
                        app.state = AppState::SelectFolder(user_path, password);
                        app.reset_selection();
                        continue;
                    }
                };

                let mut display_items = Vec::new();

                if let Some(files_obj) = metadata["folders"][folder.as_str()].as_object() {
                    for (filename, file_data) in files_obj {
                        let created_at = file_data["created_at"].as_str().unwrap_or("").to_string();
                        display_items.push((filename.clone(), created_at));
                    }
                }

                if folder == today_str() {
                    display_items.push(("Add Page".to_string(), String::new()));
                }

                let help_text = app.get_help_text();

                if let Some(NavigationResult::Selected(file)) = select_menu_with_back_and_metadata(
                    &mut terminal,
                    "Select Page",
                    &display_items,
                    &mut app.selected_index,
                    &mut app.show_help,
                    help_text,
                )? {
                    if file == "Add Page" {
                        app.state = AppState::AddPagePrompt(user_path, password, folder);
                        app.input_buffer.clear();
                    } else {
                        app.state = AppState::EditOrViewFile(user_path, password, folder, file);
                    }
                } else {
                    app.state = AppState::SelectFolder(user_path, password);
                    app.reset_selection();
                }
            }
            AppState::EditOrViewFile(user_path, password, folder, file) => {
                let file_path = app.data_dir.join(&user_path);
                let content =
                    get_file_content(&password, file_path.to_str().unwrap(), &file, &folder);
                if folder != today_str() {
                    show_message(
                        &mut terminal,
                        &format!("[READ-ONLY] Content of {}:\n\n{}", file, content),
                        "View Page",
                    )?;
                } else {
                    // Edit file using temporary file with vim/nano/default editor
                    match edit_file_with_editor(&content) {
                        Ok(new_content) => {
                            if new_content != content {
                                let file_path = app.data_dir.join(&user_path);
                                update_file_content(
                                    &password,
                                    file_path.to_str().unwrap(),
                                    &file,
                                    &folder,
                                    &new_content,
                                );
                                show_message(
                                    &mut terminal,
                                    &format!("Page '{}' updated successfully!", file),
                                    "Success",
                                )?;
                            } else {
                                show_message(&mut terminal, "No changes made to page", "Info")?;
                            }
                        }
                        Err(e) => {
                            show_message(
                                &mut terminal,
                                &format!("Error editing page: {}", e),
                                "Error",
                            )?;
                        }
                    }
                }
                app.state = AppState::SelectFile(user_path, password, folder);
                app.reset_selection();
            }
            AppState::Done => {
                show_message(
                    &mut terminal,
                    "Operation completed. Press any key to exit.",
                    "Done",
                )?;
                break;
            }
        }
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn list_clog_files(dir: &std::path::Path) -> Vec<String> {
    let mut result = vec![];
    if let Ok(paths) = fs::read_dir(dir) {
        for path in paths {
            if let Ok(path) = path {
                let path = path.path();
                if let Some(ext) = path.extension() {
                    if ext == "clog" {
                        if let Some(filename) = path.file_name() {
                            result.push(filename.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }
    result
}

fn get_user_creation_date(file_path: &std::path::Path) -> Option<String> {
    // Try to read the file and extract creation date from metadata
    if !file_path.exists() {
        return None;
    }

    // For now, we'll try to decrypt with an empty password to get basic metadata
    // In a real scenario, you might want to store unencrypted metadata separately
    // or have a different approach for getting creation dates without password

    // Try to get file system creation time as fallback
    if let Ok(metadata) = fs::metadata(file_path) {
        if let Ok(created) = metadata.created() {
            if let Ok(datetime) = created.duration_since(std::time::UNIX_EPOCH) {
                let timestamp = datetime.as_secs();
                let naive_datetime =
                    chrono::NaiveDateTime::from_timestamp_opt(timestamp as i64, 0)?;
                let datetime: chrono::DateTime<chrono::Local> =
                    chrono::DateTime::from_naive_utc_and_offset(
                        naive_datetime,
                        *chrono::Local::now().offset(),
                    );
                return Some(datetime.format("%d/%m/%Y %H:%M").to_string());
            }
        }
    }

    // If we can't get creation time, try modified time
    if let Ok(metadata) = fs::metadata(file_path) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(datetime) = modified.duration_since(std::time::UNIX_EPOCH) {
                let timestamp = datetime.as_secs();
                let naive_datetime =
                    chrono::NaiveDateTime::from_timestamp_opt(timestamp as i64, 0)?;
                let datetime: chrono::DateTime<chrono::Local> =
                    chrono::DateTime::from_naive_utc_and_offset(
                        naive_datetime,
                        *chrono::Local::now().offset(),
                    );
                return Some(datetime.format("%d/%m/%Y %H:%M").to_string());
            }
        }
    }

    None
}

fn today_str() -> String {
    Local::now().format("%d/%m/%Y").to_string()
}

#[derive(Debug)]
enum NavigationResult {
    Selected(String),
    Back,
}

fn select_menu_with_metadata(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    title: &str,
    items: &[(String, String)], // (name, metadata)
    selected_index: &mut usize,
    show_help: &mut bool,
    help_text: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    // Reduce polling frequency for better responsiveness
    let poll_duration = std::time::Duration::from_millis(16); // ~60fps

    loop {
        terminal.draw(|f| {
            let size = f.area();

            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(3)])
                .split(size);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Length(3), Constraint::Min(1)])
                .split(main_chunks[0]);

            let title_widget = Paragraph::new(title)
                .style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                );
            f.render_widget(title_widget, chunks[0]);

            if !items.is_empty() {
                let list_items: Vec<ListItem> = items
                    .iter()
                    .enumerate()
                    .map(|(i, (item, metadata))| {
                        let line = if metadata.is_empty() {
                            Line::from(vec![Span::raw(item)])
                        } else {
                            Line::from(vec![
                                Span::raw(item),
                                Span::raw(" "),
                                Span::styled(
                                    format!("[{}]", metadata),
                                    Style::default()
                                        .fg(Color::Gray)
                                        .add_modifier(Modifier::ITALIC),
                                ),
                            ])
                        };

                        if i == *selected_index {
                            ListItem::new(line).style(
                                Style::default()
                                    .bg(Color::Blue)
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            ListItem::new(line).style(Style::default().fg(Color::White))
                        }
                    })
                    .collect();

                let list = List::new(list_items)
                    .block(
                        Block::default()
                            .title("Options")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Green)),
                    )
                    .highlight_style(Style::default().add_modifier(Modifier::BOLD))
                    .highlight_symbol("► ");

                let mut state = ListState::default();
                state.select(Some(*selected_index));
                f.render_stateful_widget(list, chunks[1], &mut state);
            }

            // Help text at bottom
            let help_widget = Paragraph::new(help_text)
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Controls")
                        .border_style(Style::default().fg(Color::Yellow)),
                );
            f.render_widget(help_widget, main_chunks[1]);
        })?;

        if event::poll(poll_duration)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if *selected_index > 0 {
                            *selected_index -= 1;
                        } else {
                            *selected_index = items.len().saturating_sub(1);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if *selected_index < items.len().saturating_sub(1) {
                            *selected_index += 1;
                        } else {
                            *selected_index = 0;
                        }
                    }
                    KeyCode::Enter => {
                        if !items.is_empty() {
                            return Ok(Some(items[*selected_index].0.clone()));
                        }
                    }
                    KeyCode::Char('h') => {
                        *show_help = !*show_help;
                    }
                    KeyCode::Char('q') => std::process::exit(0),
                    _ => {}
                }
            }
        }
    }
}

fn select_menu_with_back_and_metadata(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    title: &str,
    items: &[(String, String)], // (name, metadata)
    selected_index: &mut usize,
    show_help: &mut bool,
    help_text: &str,
) -> Result<Option<NavigationResult>, Box<dyn std::error::Error>> {
    // Reduce polling frequency for better responsiveness
    let poll_duration = std::time::Duration::from_millis(16); // ~60fps

    loop {
        terminal.draw(|f| {
            let size = f.area();

            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1), Constraint::Length(3)])
                .split(size);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Length(3), Constraint::Min(1)])
                .split(main_chunks[0]);

            let title_widget = Paragraph::new(title)
                .style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                );
            f.render_widget(title_widget, chunks[0]);

            if !items.is_empty() {
                let list_items: Vec<ListItem> = items
                    .iter()
                    .enumerate()
                    .map(|(i, (item, metadata))| {
                        let line = if metadata.is_empty() {
                            Line::from(vec![Span::raw(item)])
                        } else {
                            Line::from(vec![
                                Span::raw(item),
                                Span::raw(" "),
                                Span::styled(
                                    format!("[{}]", metadata),
                                    Style::default()
                                        .fg(Color::Gray)
                                        .add_modifier(Modifier::ITALIC),
                                ),
                            ])
                        };

                        if i == *selected_index {
                            ListItem::new(line).style(
                                Style::default()
                                    .bg(Color::Blue)
                                    .fg(Color::White)
                                    .add_modifier(Modifier::BOLD),
                            )
                        } else {
                            ListItem::new(line).style(Style::default().fg(Color::White))
                        }
                    })
                    .collect();

                let list = List::new(list_items)
                    .block(
                        Block::default()
                            .title("Options")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Green)),
                    )
                    .highlight_style(Style::default().add_modifier(Modifier::BOLD))
                    .highlight_symbol("► ");

                let mut state = ListState::default();
                state.select(Some(*selected_index));
                f.render_stateful_widget(list, chunks[1], &mut state);
            } else {
                let empty_msg = Paragraph::new("No items available")
                    .style(Style::default().fg(Color::Gray))
                    .alignment(Alignment::Center)
                    .block(
                        Block::default()
                            .title("Options")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Green)),
                    );
                f.render_widget(empty_msg, chunks[1]);
            }

            // Help text at bottom
            let help_widget = Paragraph::new(help_text)
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Controls")
                        .border_style(Style::default().fg(Color::Yellow)),
                );
            f.render_widget(help_widget, main_chunks[1]);
        })?;

        if event::poll(poll_duration)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if *selected_index > 0 {
                            *selected_index -= 1;
                        } else {
                            *selected_index = items.len().saturating_sub(1);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if *selected_index < items.len().saturating_sub(1) {
                            *selected_index += 1;
                        } else {
                            *selected_index = 0;
                        }
                    }
                    KeyCode::Enter => {
                        if !items.is_empty() {
                            return Ok(Some(NavigationResult::Selected(
                                items[*selected_index].0.clone(),
                            )));
                        }
                    }
                    KeyCode::Char('b') | KeyCode::Esc => {
                        return Ok(Some(NavigationResult::Back));
                    }
                    KeyCode::Char('h') => {
                        *show_help = !*show_help;
                    }
                    KeyCode::Char('q') => std::process::exit(0),
                    _ => {}
                }
            }
        }
    }
}

fn prompt_input_in_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    prompt: &str,
    input_buffer: &mut String,
    show_help: &mut bool,
    help_text: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    input_buffer.clear();
    // Reduce polling frequency for better responsiveness
    let poll_duration = std::time::Duration::from_millis(16); // ~60fps

    loop {
        terminal.draw(|f| {
            let size = f.area();

            // Create a centered popup
            let popup_area = centered_rect(60, 20, size);

            // Clear the background
            f.render_widget(Clear, popup_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                ])
                .split(popup_area);

            // Prompt text
            let prompt_widget = Paragraph::new(prompt)
                .style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan)),
                );
            f.render_widget(prompt_widget, chunks[0]);

            // Input field
            let input_widget = Paragraph::new(input_buffer.as_str())
                .style(Style::default().fg(Color::White))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Input")
                        .border_style(Style::default().fg(Color::Green)),
                );
            f.render_widget(input_widget, chunks[1]);

            // Help text
            let help_widget = Paragraph::new(help_text)
                .style(Style::default().fg(Color::Yellow))
                .alignment(Alignment::Center)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Controls")
                        .border_style(Style::default().fg(Color::Yellow)),
                );
            f.render_widget(help_widget, chunks[2]);
        })?;

        if event::poll(poll_duration)? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char(c) => {
                        input_buffer.push(c);
                    }
                    KeyCode::Backspace => {
                        input_buffer.pop();
                    }
                    KeyCode::Enter => {
                        if !input_buffer.is_empty() {
                            return Ok(Some(input_buffer.clone()));
                        }
                    }
                    KeyCode::Esc => {
                        return Ok(None);
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(None);
                    }
                    KeyCode::Char('q') => std::process::exit(0),
                    _ => {}
                }
            }
        }
    }
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    r: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn edit_file_with_editor(content: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut temp_file = NamedTempFile::new()?;
    write!(temp_file, "{}", content)?;

    // Temporarily exit raw mode and alternate screen
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    // Try vim first, then nano, then default editor
    let editors = ["vim", "nano"];
    let mut editor_found = false;
    let mut status = None;

    for editor in &editors {
        if Command::new(editor).arg("--version").output().is_ok() {
            status = Some(Command::new(editor).arg(temp_file.path()).status()?);
            editor_found = true;
            break;
        }
    }

    // If no preferred editor found, try EDITOR env var or default
    if !editor_found {
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
        status = Some(Command::new(&editor).arg(temp_file.path()).status()?);
    }

    // Re-enter raw mode and alternate screen
    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;

    if let Some(status) = status {
        if !status.success() {
            return Err("Editor exited with non-zero status".into());
        }
    }

    let mut new_content = String::new();
    std::fs::File::open(temp_file.path())?.read_to_string(&mut new_content)?;

    Ok(new_content)
}

fn show_message(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    message: &str,
    title: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        terminal.draw(|f| {
            let size = f.area();
            let popup_area = centered_rect(80, 60, size);

            f.render_widget(Clear, popup_area);

            let block = Paragraph::new(message)
                .style(Style::default().fg(Color::White))
                .alignment(Alignment::Left)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .border_style(Style::default().fg(Color::Magenta)),
                )
                .wrap(ratatui::widgets::Wrap { trim: true });
            f.render_widget(block, popup_area);
        })?;

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(_) = event::read()? {
                break;
            }
        }
    }
    Ok(())
}
