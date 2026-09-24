mod app;
mod clipboard;
mod config;
mod db;
mod lsp;
mod models;
mod notes;
mod syntax;
mod theme;
mod vim;

use app::{App, ExecScope, FocusArea};
use clipboard::copy_text;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use lsp::LspClient;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use vim::{next_word_start, prev_word_start, word_end, VimMode};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup panic hook to restore terminal
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    // Terminal initialization FIRST (sub-millisecond!)
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(None).await;

    // Draw first frame IMMEDIATELY (sub-millisecond!)
    terminal.draw(|f| app.render(f))?;

    // Start LSP Client asynchronously in background
    let (lsp_tx, mut lsp_rx) = tokio::sync::mpsc::channel::<LspClient>(1);
    tokio::spawn(async move {
        if let Ok(client) = LspClient::spawn().await {
            let _ = lsp_tx.send(client).await;
        }
    });

    // Auto-connect to active database right after first frame is drawn
    if !app.active_conn_id.is_empty() {
        app.status_msg = "Connecting to database...".to_string();
        terminal.draw(|f| app.render(f))?;
        let active = app.active_conn_id.clone();
        if let Ok(ping) = app.db.ping(&active).await {
            app.conn_health.insert(active.clone(), (ping.online, ping.latency_ms));
        }
        app.load_structure_for_active().await;
        terminal.draw(|f| app.render(f))?;
    }

    let mut needs_redraw = false;

    // Main event loop
    loop {
        if app.lsp.is_none() {
            if let Ok(client) = lsp_rx.try_recv() {
                app.lsp = Some(client);
                needs_redraw = true;
            }
        }

        if needs_redraw {
            terminal.draw(|f| app.render(f))?;
            needs_redraw = false;
        }

        if app.should_quit {
            break;
        }

        let poll_dur = if app.is_executing || app.toast_msg.is_some() {
            Duration::from_millis(40)
        } else {
            Duration::from_millis(200)
        };

        if event::poll(poll_dur)? {
            match event::read()? {
                Event::Key(key) => {
                    needs_redraw = true;

                    // Global quit
                    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') && !app.show_copy_menu {
                        break;
                    }

                // Autocomplete Popup handling
                if app.show_completions {
                    match key.code {
                        KeyCode::Esc => {
                            app.show_completions = false;
                            continue;
                        }
                        KeyCode::Up | KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) || key.code == KeyCode::Up => {
                            if app.selected_completion > 0 {
                                app.selected_completion -= 1;
                            }
                            continue;
                        }
                        KeyCode::Down | KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) || key.code == KeyCode::Down => {
                            if app.selected_completion + 1 < app.completions.len() {
                                app.selected_completion += 1;
                            }
                            continue;
                        }
                        KeyCode::Tab | KeyCode::Enter => {
                            app.insert_completion();
                            continue;
                        }
                        _ => {
                            // Let regular editor keystrokes process
                        }
                    }
                }

                // New Connection Modal handling
                if app.show_new_conn_modal {
                    match key.code {
                        KeyCode::Esc => {
                            app.show_new_conn_modal = false;
                        }
                        KeyCode::Tab | KeyCode::Down => {
                            app.new_conn_field = (app.new_conn_field + 1) % 5;
                        }
                        KeyCode::BackTab | KeyCode::Up => {
                            app.new_conn_field = (app.new_conn_field + 4) % 5;
                        }
                        KeyCode::Left if app.new_conn_field == 1 => {
                            if app.new_conn_type_idx > 0 {
                                app.new_conn_type_idx -= 1;
                            } else {
                                app.new_conn_type_idx = app::DRIVER_TYPES.len() - 1;
                            }
                            app.new_conn_url = app::DRIVER_TYPES[app.new_conn_type_idx].2.to_string();
                        }
                        KeyCode::Right if app.new_conn_field == 1 => {
                            app.new_conn_type_idx = (app.new_conn_type_idx + 1) % app::DRIVER_TYPES.len();
                            app.new_conn_url = app::DRIVER_TYPES[app.new_conn_type_idx].2.to_string();
                        }
                        KeyCode::Char(c) => {
                            match app.new_conn_field {
                                0 => app.new_conn_name.push(c),
                                1 => {
                                    if let Some(idx) = app::DRIVER_TYPES.iter().position(|(d, _, _)| d.starts_with(c)) {
                                        app.new_conn_type_idx = idx;
                                        app.new_conn_url = app::DRIVER_TYPES[idx].2.to_string();
                                    }
                                }
                                2 => app.new_conn_url.push(c),
                                _ => {}
                            }
                        }
                        KeyCode::Backspace => {
                            match app.new_conn_field {
                                0 => { app.new_conn_name.pop(); }
                                2 => { app.new_conn_url.pop(); }
                                _ => {}
                            }
                        }
                        KeyCode::Enter => {
                            if app.new_conn_field == 4 {
                                app.show_new_conn_modal = false;
                            } else {
                                let _ = app.submit_new_connection().await;
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // Copy Menu handling
                if app.show_copy_menu {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            app.show_copy_menu = false;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.copy_menu_cursor > 0 {
                                app.copy_menu_cursor -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.copy_menu_cursor < 6 {
                                app.copy_menu_cursor += 1;
                            }
                        }
                        KeyCode::Char('1') => {
                            app.copy_current_cell();
                            app.show_copy_menu = false;
                        }
                        KeyCode::Char('2') => {
                            app.copy_current_row_json();
                            app.show_copy_menu = false;
                        }
                        KeyCode::Char('3') => {
                            app.copy_current_row_csv();
                            app.show_copy_menu = false;
                        }
                        KeyCode::Char('4') => {
                            app.copy_current_row_sql();
                            app.show_copy_menu = false;
                        }
                        KeyCode::Char('5') => {
                            app.copy_all_csv();
                            app.show_copy_menu = false;
                        }
                        KeyCode::Char('6') => {
                            app.copy_all_json();
                            app.show_copy_menu = false;
                        }
                        KeyCode::Char('7') => {
                            app.copy_all_markdown();
                            app.show_copy_menu = false;
                        }
                        KeyCode::Enter => {
                            match app.copy_menu_cursor {
                                0 => app.copy_current_cell(),
                                1 => app.copy_current_row_json(),
                                2 => app.copy_current_row_csv(),
                                3 => app.copy_current_row_sql(),
                                4 => app.copy_all_csv(),
                                5 => app.copy_all_json(),
                                6 => app.copy_all_markdown(),
                                _ => {}
                            }
                            app.show_copy_menu = false;
                        }
                        _ => {}
                    }
                    continue;
                }

                // Global execution shortcuts (available in all panes and modes)
                if key.code == KeyCode::F(5) || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r')) {
                    app.execute_query_scope(ExecScope::All).await;
                    continue;
                }

                if key.modifiers.contains(KeyModifiers::CONTROL) && (key.code == KeyCode::Enter || key.code == KeyCode::Char('j') || key.code == KeyCode::Char('e')) {
                    app.execute_query_scope(ExecScope::Statement).await;
                    continue;
                }

                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('l') {
                    app.execute_query_scope(ExecScope::Line).await;
                    continue;
                }

                // Save Note (Ctrl+S)
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
                    let note_name = app.current_note.clone().unwrap_or_else(|| format!("query_{}", chrono::Local::now().format("%Y%m%d_%H%M%S")));
                    let content = app.editor_lines.join("\n");
                    if let Ok(saved_path) = notes::save_note(&note_name, &content) {
                        app.current_note = Some(note_name);
                        app.set_toast(format!("Saved to {}", saved_path));
                        app.load_notes().await;
                        app.rebuild_flat_tree();
                    }
                    continue;
                }

                // Tab focus navigation (when not in insert mode and completions closed)
                if key.code == KeyCode::Tab && !app.show_completions && app.vim.mode != VimMode::Insert {
                    app.focus = match app.focus {
                        FocusArea::Drawer => FocusArea::Editor,
                        FocusArea::Editor => FocusArea::Results,
                        FocusArea::Results => FocusArea::Drawer,
                    };
                    continue;
                }
                if key.code == KeyCode::BackTab && !app.show_completions && app.vim.mode != VimMode::Insert {
                    app.focus = match app.focus {
                        FocusArea::Drawer => FocusArea::Results,
                        FocusArea::Editor => FocusArea::Drawer,
                        FocusArea::Results => FocusArea::Editor,
                    };
                    continue;
                }

                // Direct 1, 2, 3 focus jumping when in Normal mode or other panes
                if app.focus != FocusArea::Editor || app.vim.mode != VimMode::Insert {
                    match key.code {
                        KeyCode::Char('1') if app.focus != FocusArea::Editor => {
                            app.focus = FocusArea::Drawer;
                            continue;
                        }
                        KeyCode::Char('2') if app.focus != FocusArea::Editor => {
                            app.focus = FocusArea::Editor;
                            continue;
                        }
                        KeyCode::Char('3') if app.focus != FocusArea::Editor => {
                            app.focus = FocusArea::Results;
                            continue;
                        }
                        _ => {}
                    }
                }

                // Pane-specific keys
                match app.focus {
                    FocusArea::Drawer => {
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                let cur = app.drawer_state.selected().unwrap_or(0);
                                if cur > 0 {
                                    app.drawer_state.select(Some(cur - 1));
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let cur = app.drawer_state.selected().unwrap_or(0);
                                if cur + 1 < app.flat_nodes.len() {
                                    app.drawer_state.select(Some(cur + 1));
                                }
                            }
                            KeyCode::Enter | KeyCode::Char('s') => {
                                app.handle_drawer_enter().await;
                            }
                            KeyCode::Char('a') | KeyCode::Char('N') => {
                                app.open_new_conn_modal();
                            }
                            KeyCode::Char(' ') | KeyCode::Char('o') | KeyCode::Char('l') | KeyCode::Right => {
                                app.toggle_selected_node().await;
                            }
                            KeyCode::Char('h') | KeyCode::Left => {
                                app.toggle_selected_node().await;
                            }
                            KeyCode::Char('r') => {
                                app.load_connections().await;
                                app.load_notes().await;
                                app.rebuild_flat_tree();
                            }
                            _ => {}
                        }
                    }

                    FocusArea::Editor => {
                        match app.vim.mode {
                            VimMode::Normal => {
                                // Check pending operators (d, c, y, g)
                                if let Some(op) = app.vim.pending_op.take() {
                                    match (op, key.code) {
                                        ('g', KeyCode::Char('g')) => {
                                            app.cursor_row = 0;
                                            app.cursor_col = 0;
                                        }
                                        ('d', KeyCode::Char('d')) => {
                                            app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                            if !app.editor_lines.is_empty() {
                                                let line = app.editor_lines.remove(app.cursor_row);
                                                app.vim.register = line;
                                                app.vim.is_line_register = true;
                                                if app.editor_lines.is_empty() {
                                                    app.editor_lines.push(String::new());
                                                }
                                                app.cursor_row = app.cursor_row.min(app.editor_lines.len() - 1);
                                                app.cursor_col = app.cursor_col.min(app.editor_lines[app.cursor_row].len().saturating_sub(1));
                                            }
                                        }
                                        ('d', KeyCode::Char('w')) => {
                                            app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                            let cur_line = &app.editor_lines[app.cursor_row];
                                            let next_w = next_word_start(cur_line, app.cursor_col);
                                            let cut = &cur_line[app.cursor_col..next_w];
                                            app.vim.register = cut.to_string();
                                            app.vim.is_line_register = false;
                                            let new_line = format!("{}{}", &cur_line[..app.cursor_col], &cur_line[next_w..]);
                                            app.editor_lines[app.cursor_row] = new_line;
                                        }
                                        ('d', KeyCode::Char('$')) => {
                                            app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                            let cur_line = &app.editor_lines[app.cursor_row];
                                            let cut = &cur_line[app.cursor_col..];
                                            app.vim.register = cut.to_string();
                                            app.vim.is_line_register = false;
                                            app.editor_lines[app.cursor_row] = cur_line[..app.cursor_col].to_string();
                                        }
                                        ('c', KeyCode::Char('c')) => {
                                            app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                            app.editor_lines[app.cursor_row] = String::new();
                                            app.cursor_col = 0;
                                            app.vim.enter_insert(&app.editor_lines, app.cursor_row, app.cursor_col);
                                        }
                                        ('c', KeyCode::Char('w')) => {
                                            app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                            let cur_line = &app.editor_lines[app.cursor_row];
                                            let next_w = next_word_start(cur_line, app.cursor_col);
                                            let new_line = format!("{}{}", &cur_line[..app.cursor_col], &cur_line[next_w..]);
                                            app.editor_lines[app.cursor_row] = new_line;
                                            app.vim.enter_insert(&app.editor_lines, app.cursor_row, app.cursor_col);
                                        }
                                        ('y', KeyCode::Char('y')) => {
                                            let line = app.editor_lines[app.cursor_row].clone();
                                            app.vim.register = line.clone();
                                            app.vim.is_line_register = true;
                                            copy_text(&line);
                                            app.set_toast("Yanked 1 line".to_string());
                                        }
                                        ('y', KeyCode::Char('w')) => {
                                            let cur_line = &app.editor_lines[app.cursor_row];
                                            let next_w = next_word_start(cur_line, app.cursor_col);
                                            let cut = &cur_line[app.cursor_col..next_w];
                                            app.vim.register = cut.to_string();
                                            app.vim.is_line_register = false;
                                            copy_text(cut);
                                            app.set_toast(format!("Yanked word: \"{}\"", cut));
                                        }
                                        _ => {}
                                    }
                                    continue;
                                }

                                match key.code {
                                    // Mode switches
                                    KeyCode::Char('i') => {
                                        app.vim.enter_insert(&app.editor_lines, app.cursor_row, app.cursor_col);
                                    }
                                    KeyCode::Char('I') => {
                                        let cur_line = &app.editor_lines[app.cursor_row];
                                        let first_non_blank = cur_line.find(|c: char| !c.is_whitespace()).unwrap_or(0);
                                        app.cursor_col = first_non_blank;
                                        app.vim.enter_insert(&app.editor_lines, app.cursor_row, app.cursor_col);
                                    }
                                    KeyCode::Char('a') => {
                                        let cur_len = app.editor_lines[app.cursor_row].len();
                                        if cur_len > 0 && app.cursor_col < cur_len {
                                            app.cursor_col += 1;
                                        }
                                        app.vim.enter_insert(&app.editor_lines, app.cursor_row, app.cursor_col);
                                    }
                                    KeyCode::Char('A') => {
                                        app.cursor_col = app.editor_lines[app.cursor_row].len();
                                        app.vim.enter_insert(&app.editor_lines, app.cursor_row, app.cursor_col);
                                    }
                                    KeyCode::Char('o') => {
                                        app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                        app.editor_lines.insert(app.cursor_row + 1, String::new());
                                        app.cursor_row += 1;
                                        app.cursor_col = 0;
                                        app.vim.enter_insert(&app.editor_lines, app.cursor_row, app.cursor_col);
                                    }
                                    KeyCode::Char('O') => {
                                        app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                        app.editor_lines.insert(app.cursor_row, String::new());
                                        app.cursor_col = 0;
                                        app.vim.enter_insert(&app.editor_lines, app.cursor_row, app.cursor_col);
                                    }
                                    KeyCode::Char('v') => {
                                        app.vim.enter_visual(app.cursor_row, app.cursor_col, false);
                                    }
                                    KeyCode::Char('V') => {
                                        app.vim.enter_visual(app.cursor_row, app.cursor_col, true);
                                    }

                                    // Motions
                                    KeyCode::Char('h') | KeyCode::Left => {
                                        app.cursor_col = app.cursor_col.saturating_sub(1);
                                    }
                                    KeyCode::Char('l') | KeyCode::Right => {
                                        let max_c = app.editor_lines[app.cursor_row].len().saturating_sub(1);
                                        if app.cursor_col < max_c {
                                            app.cursor_col += 1;
                                        }
                                    }
                                    KeyCode::Char('j') | KeyCode::Down => {
                                        if app.cursor_row + 1 < app.editor_lines.len() {
                                            app.cursor_row += 1;
                                            app.cursor_col = app.cursor_col.min(app.editor_lines[app.cursor_row].len().saturating_sub(1));
                                        }
                                    }
                                    KeyCode::Char('k') | KeyCode::Up => {
                                        if app.cursor_row > 0 {
                                            app.cursor_row -= 1;
                                            app.cursor_col = app.cursor_col.min(app.editor_lines[app.cursor_row].len().saturating_sub(1));
                                        }
                                    }
                                    KeyCode::Char('w') => {
                                        let cur_line = &app.editor_lines[app.cursor_row];
                                        app.cursor_col = next_word_start(cur_line, app.cursor_col);
                                    }
                                    KeyCode::Char('b') => {
                                        let cur_line = &app.editor_lines[app.cursor_row];
                                        app.cursor_col = prev_word_start(cur_line, app.cursor_col);
                                    }
                                    KeyCode::Char('e') => {
                                        let cur_line = &app.editor_lines[app.cursor_row];
                                        app.cursor_col = word_end(cur_line, app.cursor_col);
                                    }
                                    KeyCode::Char('0') | KeyCode::Home => {
                                        app.cursor_col = 0;
                                    }
                                    KeyCode::Char('$') | KeyCode::End => {
                                        app.cursor_col = app.editor_lines[app.cursor_row].len().saturating_sub(1);
                                    }
                                    KeyCode::Char('^') => {
                                        let cur_line = &app.editor_lines[app.cursor_row];
                                        app.cursor_col = cur_line.find(|c: char| !c.is_whitespace()).unwrap_or(0);
                                    }
                                    KeyCode::Char('G') => {
                                        app.cursor_row = app.editor_lines.len().saturating_sub(1);
                                        app.cursor_col = 0;
                                    }
                                    KeyCode::Char('g') => {
                                        app.vim.pending_op = Some('g');
                                    }

                                    // Operators
                                    KeyCode::Char('x') => {
                                        app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                        let cur_line = &mut app.editor_lines[app.cursor_row];
                                        if app.cursor_col < cur_line.len() {
                                            let ch = cur_line.remove(app.cursor_col);
                                            app.vim.register = ch.to_string();
                                            app.vim.is_line_register = false;
                                            app.cursor_col = app.cursor_col.min(cur_line.len().saturating_sub(1));
                                        }
                                    }
                                    // Half-page scrolls
                                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        app.cursor_row = (app.cursor_row + 8).min(app.editor_lines.len().saturating_sub(1));
                                    }
                                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        app.cursor_row = app.cursor_row.saturating_sub(8);
                                    }

                                    KeyCode::Char('d') => {
                                        app.vim.pending_op = Some('d');
                                    }
                                    KeyCode::Char('c') => {
                                        app.vim.pending_op = Some('c');
                                    }
                                    KeyCode::Char('y') => {
                                        app.vim.pending_op = Some('y');
                                    }
                                    KeyCode::Char('p') => {
                                        if !app.vim.register.is_empty() {
                                            app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                            if app.vim.is_line_register {
                                                app.editor_lines.insert(app.cursor_row + 1, app.vim.register.clone());
                                                app.cursor_row += 1;
                                                app.cursor_col = 0;
                                            } else {
                                                let cur_line = &mut app.editor_lines[app.cursor_row];
                                                let pos = (app.cursor_col + 1).min(cur_line.len());
                                                cur_line.insert_str(pos, &app.vim.register);
                                                app.cursor_col = pos + app.vim.register.len().saturating_sub(1);
                                            }
                                        }
                                    }
                                    KeyCode::Char('P') => {
                                        if !app.vim.register.is_empty() {
                                            app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                            if app.vim.is_line_register {
                                                app.editor_lines.insert(app.cursor_row, app.vim.register.clone());
                                                app.cursor_col = 0;
                                            } else {
                                                let cur_line = &mut app.editor_lines[app.cursor_row];
                                                cur_line.insert_str(app.cursor_col, &app.vim.register);
                                            }
                                        }
                                    }
                                    KeyCode::Char('u') => {
                                        if app.vim.undo(&mut app.editor_lines, &mut app.cursor_row, &mut app.cursor_col) {
                                            app.set_toast("Undo".to_string());
                                        }
                                    }

                                    // Execute Statement on Enter in Normal Mode
                                    KeyCode::Enter => {
                                        app.execute_query_scope(ExecScope::Statement).await;
                                    }
                                    _ => {}
                                }
                            }

                            VimMode::Visual | VimMode::VisualLine => {
                                match key.code {
                                    KeyCode::Esc => {
                                        app.vim.enter_normal(app.cursor_row, &mut app.cursor_col, &app.editor_lines);
                                    }
                                    KeyCode::Char('h') | KeyCode::Left => {
                                        app.cursor_col = app.cursor_col.saturating_sub(1);
                                    }
                                    KeyCode::Char('l') | KeyCode::Right => {
                                        let max_c = app.editor_lines[app.cursor_row].len().saturating_sub(1);
                                        if app.cursor_col < max_c {
                                            app.cursor_col += 1;
                                        }
                                    }
                                    KeyCode::Char('j') | KeyCode::Down => {
                                        if app.cursor_row + 1 < app.editor_lines.len() {
                                            app.cursor_row += 1;
                                            app.cursor_col = app.cursor_col.min(app.editor_lines[app.cursor_row].len().saturating_sub(1));
                                        }
                                    }
                                    KeyCode::Char('k') | KeyCode::Up => {
                                        if app.cursor_row > 0 {
                                            app.cursor_row -= 1;
                                            app.cursor_col = app.cursor_col.min(app.editor_lines[app.cursor_row].len().saturating_sub(1));
                                        }
                                    }
                                    KeyCode::Char('y') => {
                                        if let Some((anchor_r, _)) = app.vim.visual_anchor {
                                            let (r_start, r_end) = if anchor_r <= app.cursor_row { (anchor_r, app.cursor_row) } else { (app.cursor_row, anchor_r) };
                                            let text = app.editor_lines[r_start..=r_end].join("\n");
                                            app.vim.register = text.clone();
                                            app.vim.is_line_register = true;
                                            copy_text(&text);
                                            app.set_toast(format!("Yanked {} lines", r_end - r_start + 1));
                                        }
                                        app.vim.enter_normal(app.cursor_row, &mut app.cursor_col, &app.editor_lines);
                                    }
                                    KeyCode::Char('d') | KeyCode::Char('x') => {
                                        app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                        if let Some((anchor_r, _)) = app.vim.visual_anchor {
                                            let (r_start, r_end) = if anchor_r <= app.cursor_row { (anchor_r, app.cursor_row) } else { (app.cursor_row, anchor_r) };
                                            let removed: Vec<String> = app.editor_lines.drain(r_start..=r_end).collect();
                                            app.vim.register = removed.join("\n");
                                            app.vim.is_line_register = true;
                                            if app.editor_lines.is_empty() {
                                                app.editor_lines.push(String::new());
                                            }
                                            app.cursor_row = r_start.min(app.editor_lines.len() - 1);
                                            app.cursor_col = 0;
                                        }
                                        app.vim.enter_normal(app.cursor_row, &mut app.cursor_col, &app.editor_lines);
                                    }
                                    KeyCode::Char('c') => {
                                        app.vim.save_undo(&app.editor_lines, app.cursor_row, app.cursor_col);
                                        if let Some((anchor_r, _)) = app.vim.visual_anchor {
                                            let (r_start, r_end) = if anchor_r <= app.cursor_row { (anchor_r, app.cursor_row) } else { (app.cursor_row, anchor_r) };
                                            app.editor_lines.drain(r_start..=r_end);
                                            app.editor_lines.insert(r_start, String::new());
                                            app.cursor_row = r_start;
                                            app.cursor_col = 0;
                                        }
                                        app.vim.enter_insert(&app.editor_lines, app.cursor_row, app.cursor_col);
                                    }
                                    _ => {}
                                }
                            }

                            VimMode::Insert => {
                                // Trigger autocompletion on Ctrl+Space
                                if key.modifiers.contains(KeyModifiers::CONTROL) && (key.code == KeyCode::Char(' ') || key.code == KeyCode::Null) {
                                    app.trigger_completions(true).await;
                                    continue;
                                }

                                match key.code {
                                    KeyCode::Esc => {
                                        app.vim.enter_normal(app.cursor_row, &mut app.cursor_col, &app.editor_lines);
                                        app.show_completions = false;
                                    }
                                    KeyCode::Up => {
                                        if app.cursor_row > 0 {
                                            app.cursor_row -= 1;
                                            app.cursor_col = app.cursor_col.min(app.editor_lines[app.cursor_row].len());
                                        }
                                        app.show_completions = false;
                                    }
                                    KeyCode::Down => {
                                        if app.cursor_row + 1 < app.editor_lines.len() {
                                            app.cursor_row += 1;
                                            app.cursor_col = app.cursor_col.min(app.editor_lines[app.cursor_row].len());
                                        }
                                        app.show_completions = false;
                                    }
                                    KeyCode::Left => {
                                        if app.cursor_col > 0 {
                                            app.cursor_col -= 1;
                                        } else if app.cursor_row > 0 {
                                            app.cursor_row -= 1;
                                            app.cursor_col = app.editor_lines[app.cursor_row].len();
                                        }
                                        app.show_completions = false;
                                    }
                                    KeyCode::Right => {
                                        if app.cursor_col < app.editor_lines[app.cursor_row].len() {
                                            app.cursor_col += 1;
                                        } else if app.cursor_row + 1 < app.editor_lines.len() {
                                            app.cursor_row += 1;
                                            app.cursor_col = 0;
                                        }
                                        app.show_completions = false;
                                    }
                                    KeyCode::Home => {
                                        app.cursor_col = 0;
                                        app.show_completions = false;
                                    }
                                    KeyCode::End => {
                                        app.cursor_col = app.editor_lines[app.cursor_row].len();
                                        app.show_completions = false;
                                    }
                                    KeyCode::Enter => {
                                        let cur_line = app.editor_lines[app.cursor_row].clone();
                                        let col = app.cursor_col.min(cur_line.len());
                                        let before = cur_line[..col].to_string();
                                        let after = cur_line[col..].to_string();

                                        app.editor_lines[app.cursor_row] = before;
                                        app.editor_lines.insert(app.cursor_row + 1, after);
                                        app.cursor_row += 1;
                                        app.cursor_col = 0;
                                        app.show_completions = false;
                                    }
                                    KeyCode::Backspace => {
                                        if app.cursor_col > 0 {
                                            let mut line = app.editor_lines[app.cursor_row].clone();
                                            line.remove(app.cursor_col - 1);
                                            app.editor_lines[app.cursor_row] = line;
                                            app.cursor_col -= 1;
                                            app.trigger_completions(false).await;
                                        } else if app.cursor_row > 0 {
                                            let cur_line = app.editor_lines.remove(app.cursor_row);
                                            app.cursor_row -= 1;
                                            app.cursor_col = app.editor_lines[app.cursor_row].len();
                                            app.editor_lines[app.cursor_row].push_str(&cur_line);
                                            app.show_completions = false;
                                        }
                                    }
                                    KeyCode::Delete => {
                                        let line_len = app.editor_lines[app.cursor_row].len();
                                        if app.cursor_col < line_len {
                                            let mut line = app.editor_lines[app.cursor_row].clone();
                                            line.remove(app.cursor_col);
                                            app.editor_lines[app.cursor_row] = line;
                                        } else if app.cursor_row + 1 < app.editor_lines.len() {
                                            let next_line = app.editor_lines.remove(app.cursor_row + 1);
                                            app.editor_lines[app.cursor_row].push_str(&next_line);
                                        }
                                    }
                                    KeyCode::Tab => {
                                        let mut line = app.editor_lines[app.cursor_row].clone();
                                        line.insert_str(app.cursor_col, "  ");
                                        app.editor_lines[app.cursor_row] = line;
                                        app.cursor_col += 2;
                                    }
                                    KeyCode::Char(c) => {
                                        let mut line = app.editor_lines[app.cursor_row].clone();
                                        line.insert(app.cursor_col, c);
                                        app.editor_lines[app.cursor_row] = line;
                                        app.cursor_col += 1;
                                        app.trigger_completions(false).await;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    FocusArea::Results => {
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => {
                                if let Some(_) = &app.query_result {
                                    let cur = app.table_state.selected().unwrap_or(0);
                                    if cur > 0 {
                                        app.table_state.select(Some(cur - 1));
                                    }
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if let Some(res) = &app.query_result {
                                    let cur = app.table_state.selected().unwrap_or(0);
                                    if cur + 1 < res.rows.len() {
                                        app.table_state.select(Some(cur + 1));
                                    }
                                }
                            }
                            KeyCode::Right | KeyCode::Char('l') => {
                                if let Some(res) = &app.query_result {
                                    if app.selected_col + 1 < res.headers.len() {
                                        app.selected_col += 1;
                                    }
                                }
                            }
                            KeyCode::Left | KeyCode::Char('h') => {
                                if app.selected_col > 0 {
                                    app.selected_col -= 1;
                                }
                            }
                            KeyCode::Char('L') | KeyCode::Char(']') => {
                                if let Some(res) = &app.query_result {
                                    app.selected_col = (app.selected_col + 4).min(res.headers.len().saturating_sub(1));
                                }
                            }
                            KeyCode::Char('H') | KeyCode::Char('[') => {
                                app.selected_col = app.selected_col.saturating_sub(4);
                            }
                            KeyCode::Char('c') | KeyCode::Enter => {
                                if app.query_result.is_some() {
                                    app.show_copy_menu = true;
                                    app.copy_menu_cursor = 0;
                                }
                            }
                            KeyCode::Char('y') => {
                                app.copy_current_cell();
                            }
                            KeyCode::Char('Y') => {
                                app.copy_current_row_json();
                            }
                            KeyCode::PageUp => {
                                if let Some(_) = &app.query_result {
                                    let cur = app.table_state.selected().unwrap_or(0);
                                    app.table_state.select(Some(cur.saturating_sub(8)));
                                }
                            }
                            KeyCode::PageDown => {
                                if let Some(res) = &app.query_result {
                                    let cur = app.table_state.selected().unwrap_or(0);
                                    let next = (cur + 8).min(res.rows.len().saturating_sub(1));
                                    app.table_state.select(Some(next));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Event::Resize(_, _) => {
                needs_redraw = true;
            }
            _ => {}
        }
    } else if app.is_executing || app.toast_msg.is_some() {
        needs_redraw = true;
    }
}

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
