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
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
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
    InputPrompt(String, Box<AppState>),
    AddPagePrompt(String, String, String),
    Done,
}

struct App {
    state: AppState,
    selected_index: usize,
    input_buffer: String,
    data_dir: PathBuf,
    last_frame: Instant,
}

impl App {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let project_dirs =
            ProjectDirs::from("com", "levi", "clog").ok_or("Failed to get project directories")?;
        let data_dir = project_dirs.data_dir().to_path_buf();
        fs::create_dir_all(&data_dir)?;

        Ok(Self {
            state: AppState::SelectUser,
            selected_index: 0,
            input_buffer: String::new(),
            data_dir,
            last_frame: Instant::now(),
        })
    }

    fn reset_selection(&mut self) {
        self.selected_index = 0;
    }

    fn get_help_text(&self) -> &'static str {
        match &self.state {
            AppState::SelectUser => "↑/↓ or j/k: Navigate | Enter: Select | q: Quit",
            AppState::EnterNewUser | AppState::EnterPassword(_) => {
                "Enter when prompted | Esc: Back | q: Quit"
            }
            AppState::SelectFolder(_, _) | AppState::SelectFile(_, _, _) => {
                "↑/↓ or j/k: Navigate | Enter: Select | b/Esc: Back | q: Quit"
            }
            AppState::EditOrViewFile(_, _, _, _) => "Page will open in editor | q: Quit",
            AppState::InputPrompt(_, _) | AppState::AddPagePrompt(_, _, _) => {
                "Type input | Enter: Confirm | Esc: Cancel"
            }
            AppState::Done => "Press any key to exit",
        }
    }

    fn should_render(&mut self) -> bool {
        let now = Instant::now();
        let frame_duration = Duration::from_millis(16); // 60 FPS

        if now.duration_since(self.last_frame) >= frame_duration {
            self.last_frame = now;
            true
        } else {
            false
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
        if app.should_render() {
            terminal.clear()?;
        }

        let current_state = app.state.clone();
        match current_state {
            AppState::SelectUser => {
                let user_files = list_clog_files(&app.data_dir);
                let mut display_items: Vec<(String, String)> = user_files
                    .iter()
                    .map(|file| {
                        let file_path = app.data_dir.join(file);
                        let date = get_user_creation_date(&file_path).unwrap_or_default();
                        (file.clone(), date)
                    })
                    .collect();

                display_items.push(("Add New User".to_string(), String::new()));

                let help_text = app.get_help_text();
                let mut selected_index = app.selected_index;
                if let Some(selection) = select_menu_with_metadata(
                    &mut terminal,
                    "Select User",
                    &display_items,
                    &mut selected_index,
                    &help_text,
                    &mut app,
                )? {
                    app.selected_index = selected_index;
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
            AppState::InputPrompt(prompt, next_state) => {
                let help_text = app.get_help_text();
                let mut input_buffer = app.input_buffer.clone();
                if let Some(input) = prompt_input_in_app(
                    &mut terminal,
                    &prompt,
                    &mut input_buffer,
                    help_text,
                    &mut app,
                )? {
                    app.input_buffer = input_buffer;
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
                            match std::panic::catch_unwind(|| {
                                get_json_metadata(&password, file_path.to_str().unwrap())
                            }) {
                                Ok(_) => {
                                    app.state = AppState::SelectFolder(user_path, password);
                                    app.reset_selection();
                                }
                                Err(_) => {
                                    show_message(
                                        &mut terminal,
                                        "Incorrect password!",
                                        "Error",
                                        &mut app,
                                    )?;
                                    app.state = AppState::SelectUser;
                                    app.reset_selection();
                                }
                            }
                        }
                        AppState::SelectFolder(user_path, _) => {
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
                    app.input_buffer = input_buffer;
                    app.state = AppState::SelectUser;
                    app.reset_selection();
                    app.input_buffer.clear();
                }
            }
            AppState::AddPagePrompt(user_path, password, folder) => {
                let help_text = app.get_help_text();
                let mut input_buffer = app.input_buffer.clone();
                if let Some(filename) = prompt_input_in_app(
                    &mut terminal,
                    "Enter page name:",
                    &mut input_buffer,
                    help_text,
                    &mut app,
                )? {
                    app.input_buffer = input_buffer;
                    match edit_file_with_editor("") {
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
                                    &mut app,
                                )?;
                            } else {
                                show_message(
                                    &mut terminal,
                                    "Page creation cancelled (empty content)",
                                    "Info",
                                    &mut app,
                                )?;
                            }
                        }
                        Err(e) => {
                            show_message(
                                &mut terminal,
                                &format!("Error creating page: {}", e),
                                "Error",
                                &mut app,
                            )?;
                        }
                    }
                    app.state = AppState::SelectFile(user_path, password, folder);
                    app.reset_selection();
                    app.input_buffer.clear();
                } else {
                    app.input_buffer = input_buffer;
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
                        show_message(&mut terminal, "Error parsing metadata", "Error", &mut app)?;
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

                let display_items: Vec<(String, String)> = folders
                    .into_iter()
                    .map(|folder| (folder, String::new()))
                    .collect();

                let help_text = app.get_help_text();
                let mut selected_index = app.selected_index;
                if let Some(NavigationResult::Selected(folder)) =
                    select_menu_with_back_and_metadata(
                        &mut terminal,
                        "Select Chapter",
                        &display_items,
                        &mut selected_index,
                        help_text,
                        &mut app,
                    )?
                {
                    app.selected_index = selected_index;
                    app.state = AppState::SelectFile(user_path, password, folder);
                    app.reset_selection();
                } else {
                    app.selected_index = selected_index;
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
                        show_message(&mut terminal, "Error parsing metadata", "Error", &mut app)?;
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

                let today_string = today_str();
                if folder == today_string {
                    display_items.push(("Add Page".to_string(), String::new()));
                }

                let help_text = app.get_help_text();
                let mut selected_index = app.selected_index;
                if let Some(NavigationResult::Selected(file)) = select_menu_with_back_and_metadata(
                    &mut terminal,
                    "Select Page",
                    &display_items,
                    &mut selected_index,
                    help_text,
                    &mut app,
                )? {
                    app.selected_index = selected_index;
                    if file == "Add Page" {
                        app.state = AppState::AddPagePrompt(user_path, password, folder);
                        app.input_buffer.clear();
                    } else {
                        app.state = AppState::EditOrViewFile(user_path, password, folder, file);
                    }
                } else {
                    app.selected_index = selected_index;
                    app.state = AppState::SelectFolder(user_path, password);
                    app.reset_selection();
                }
            }
            AppState::EditOrViewFile(user_path, password, folder, file) => {
                let file_path = app.data_dir.join(&user_path);
                let content =
                    get_file_content(&password, file_path.to_str().unwrap(), &file, &folder);

                let today_string = today_str();
                if folder != today_string {
                    show_message(
                        &mut terminal,
                        &format!("[READ-ONLY] Content of {}:\n\n{}", file, content),
                        "View Page",
                        &mut app,
                    )?;
                } else {
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
                                    &mut app,
                                )?;
                            } else {
                                show_message(
                                    &mut terminal,
                                    "No changes made to page",
                                    "Info",
                                    &mut app,
                                )?;
                            }
                        }
                        Err(e) => {
                            show_message(
                                &mut terminal,
                                &format!("Error editing page: {}", e),
                                "Error",
                                &mut app,
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
                    &mut app,
                )?;
                break;
            }
            _ => unreachable!(),
        }
    }
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

fn list_clog_files(dir: &std::path::Path) -> Vec<String> {
    let mut result = vec![];
    if let Ok(paths) = fs::read_dir(dir) {
        for path in paths.flatten() {
            let path = path.path();
            if path.extension().map_or(false, |ext| ext == "clog") {
                if let Some(filename) = path.file_name() {
                    result.push(filename.to_string_lossy().to_string());
                }
            }
        }
    }
    result
}

fn get_user_creation_date(file_path: &std::path::Path) -> Option<String> {
    if !file_path.exists() {
        return None;
    }

    let metadata = fs::metadata(file_path).ok()?;
    let time = metadata.created().or_else(|_| metadata.modified()).ok()?;
    let datetime = time.duration_since(std::time::UNIX_EPOCH).ok()?;
    let timestamp = datetime.as_secs();
    let naive_datetime = chrono::NaiveDateTime::from_timestamp_opt(timestamp as i64, 0)?;
    let datetime: chrono::DateTime<chrono::Local> =
        chrono::DateTime::from_naive_utc_and_offset(naive_datetime, *chrono::Local::now().offset());
    Some(datetime.format("%d/%m/%Y %H:%M").to_string())
}

fn today_str() -> String {
    Local::now().format("%d/%m/%Y").to_string()
}

#[derive(Debug)]
enum NavigationResult {
    Selected(String),
    Back,
}

fn render_menu_ui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    title: &str,
    items: &[(String, String)],
    selected_index: usize,
    help_text: &str,
    show_back: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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

                    if i == selected_index {
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
            state.select(Some(selected_index));
            f.render_stateful_widget(list, chunks[1], &mut state);
        } else if show_back {
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
    Ok(())
}

fn handle_menu_input(
    selected_index: &mut usize,
    items_len: usize,
    allow_back: bool,
) -> Result<Option<MenuAction>, Box<dyn std::error::Error>> {
    if event::poll(Duration::from_millis(16))? {
        if let Event::Key(key) = event::read()? {
            // Fix Windows double keypress issue
            if key.kind != KeyEventKind::Press {
                return Ok(None);
            }

            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected_index > 0 {
                        *selected_index -= 1;
                    } else {
                        *selected_index = items_len.saturating_sub(1);
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected_index < items_len.saturating_sub(1) {
                        *selected_index += 1;
                    } else {
                        *selected_index = 0;
                    }
                }
                KeyCode::Enter => {
                    if items_len > 0 {
                        return Ok(Some(MenuAction::Select));
                    }
                }
                KeyCode::Char('b') | KeyCode::Esc if allow_back => {
                    return Ok(Some(MenuAction::Back));
                }
                KeyCode::Char('q') => std::process::exit(0),
                _ => {}
            }
        }
    }
    Ok(None)
}

enum MenuAction {
    Select,
    Back,
}

fn select_menu_with_metadata(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    title: &str,
    items: &[(String, String)],
    selected_index: &mut usize,
    help_text: &str,
    app: &mut App,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    loop {
        if app.should_render() {
            render_menu_ui(terminal, title, items, *selected_index, help_text, false)?;
        }

        if let Some(action) = handle_menu_input(selected_index, items.len(), false)? {
            match action {
                MenuAction::Select => {
                    if !items.is_empty() {
                        return Ok(Some(items[*selected_index].0.clone()));
                    }
                }
                MenuAction::Back => {} // Not used in this function
            }
        }
    }
}

fn select_menu_with_back_and_metadata(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    title: &str,
    items: &[(String, String)],
    selected_index: &mut usize,
    help_text: &str,
    app: &mut App,
) -> Result<Option<NavigationResult>, Box<dyn std::error::Error>> {
    loop {
        if app.should_render() {
            render_menu_ui(terminal, title, items, *selected_index, help_text, true)?;
        }

        if let Some(action) = handle_menu_input(selected_index, items.len(), true)? {
            match action {
                MenuAction::Select => {
                    if !items.is_empty() {
                        return Ok(Some(NavigationResult::Selected(
                            items[*selected_index].0.clone(),
                        )));
                    }
                }
                MenuAction::Back => {
                    return Ok(Some(NavigationResult::Back));
                }
            }
        }
    }
}

fn prompt_input_in_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    prompt: &str,
    input_buffer: &mut String,
    help_text: &str,
    app: &mut App,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    input_buffer.clear();

    loop {
        if app.should_render() {
            terminal.draw(|f| {
                let size = f.area();
                let popup_area = centered_rect(80, 80, size);
                f.render_widget(Clear, popup_area);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Length(3),
                        Constraint::Length(3),
                    ])
                    .split(popup_area);

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

                let input_widget = Paragraph::new(input_buffer.as_str())
                    .style(Style::default().fg(Color::White))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Input")
                            .border_style(Style::default().fg(Color::Green)),
                    );
                f.render_widget(input_widget, chunks[1]);

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
        }

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                // Fix Windows double keypress issue
                if key.kind != KeyEventKind::Press {
                    continue;
                }

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
    // Create temp file but keep it persistent
    let mut temp_file = NamedTempFile::new()?;
    let temp_path = temp_file.path().to_path_buf();

    // Write content and flush to ensure it's written
    write!(temp_file, "{}", content)?;
    temp_file.flush()?;

    // Convert temp file to persistent file to avoid handle issues
    let persistent_path = temp_file.into_temp_path();

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    let editors = if cfg!(windows) {
        // Use full path for notepad and add more Windows editors
        vec!["notepad.exe", "code", "notepad++", "vim", "nano"]
    } else {
        vec!["vim", "nano", "vi", "emacs"]
    };

    let mut editor_found = false;
    let mut status = None;

    for editor in &editors {
        // Special handling for notepad
        if editor == &"notepad.exe" {
            status = Some(Command::new("notepad.exe").arg(&persistent_path).status()?);
            editor_found = true;
            break;
        } else {
            // Check if other editors exist
            if Command::new(editor).arg("--version").output().is_ok() {
                status = Some(Command::new(editor).arg(&persistent_path).status()?);
                editor_found = true;
                break;
            }
        }
    }

    // Fallback to environment variable or default
    if !editor_found {
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| {
            if cfg!(windows) {
                "notepad.exe".to_string()
            } else {
                "vi".to_string()
            }
        });
        status = Some(Command::new(&editor).arg(&persistent_path).status()?);
    }

    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;

    if let Some(status) = status {
        if !status.success() {
            return Err("Editor exited with non-zero status".into());
        }
    }

    // Read the modified content
    let mut new_content = String::new();
    std::fs::File::open(&persistent_path)?.read_to_string(&mut new_content)?;

    // Clean up the temporary file
    std::fs::remove_file(&persistent_path).ok(); // Ignore errors on cleanup

    Ok(new_content)
}

// Alternative approach using a regular file in temp directory
fn edit_file_with_editor_alt(content: &str) -> Result<String, Box<dyn std::error::Error>> {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Create a unique filename in temp directory
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("rust_editor_{}.txt", timestamp));

    // Write content to file
    std::fs::write(&temp_file_path, content)?;

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    // Launch notepad
    let status = if cfg!(windows) {
        Command::new("notepad.exe").arg(&temp_file_path).status()?
    } else {
        // Unix fallback
        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
        Command::new(&editor).arg(&temp_file_path).status()?
    };

    execute!(io::stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;

    if !status.success() {
        std::fs::remove_file(&temp_file_path).ok();
        return Err("Editor exited with non-zero status".into());
    }

    // Read modified content
    let new_content = std::fs::read_to_string(&temp_file_path)?;

    // Clean up
    std::fs::remove_file(&temp_file_path).ok();

    Ok(new_content)
}
fn show_message(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    message: &str,
    title: &str,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        if app.should_render() {
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
        }

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                // Fix Windows double keypress issue
                if key.kind == KeyEventKind::Press {
                    break;
                }
            }
        }
    }
    Ok(())
}
