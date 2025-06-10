use crate::irc_server::{CustomMessage, ServerLog};
use color_eyre::Result;
use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{prelude::*, widgets::*};
use std::collections::VecDeque;
use tokio::sync::mpsc;

const MAX_LOGS: usize = 100;

// Expanded InputFocus
enum InputFocus {
    Channel,
    Username,
    DisplayName,
    Color,
    Message,
}

impl InputFocus {
    fn next(&self) -> Self {
        match self {
            InputFocus::Channel => InputFocus::Username,
            InputFocus::Username => InputFocus::DisplayName,
            InputFocus::DisplayName => InputFocus::Color,
            InputFocus::Color => InputFocus::Message,
            InputFocus::Message => InputFocus::Channel,
        }
    }
    fn prev(&self) -> Self {
        match self {
            InputFocus::Channel => InputFocus::Message,
            InputFocus::Username => InputFocus::Channel,
            InputFocus::DisplayName => InputFocus::Username,
            InputFocus::Color => InputFocus::DisplayName,
            InputFocus::Message => InputFocus::Color,
        }
    }
}

pub struct App {
    logs: VecDeque<String>,
    log_rx: mpsc::Receiver<ServerLog>,
    custom_msg_tx: mpsc::Sender<CustomMessage>,
    should_quit: bool,
    input_channel: String,
    input_username: String,
    input_display_name: String,
    input_color: String,
    input_message: String,
    current_focus: InputFocus,
    log_list_state: ListState,
}

impl App {
    pub fn new(
        log_rx: mpsc::Receiver<ServerLog>,
        custom_msg_tx: mpsc::Sender<CustomMessage>,
    ) -> Self {
        let default_username = "testuser".to_string();
        App {
            logs: VecDeque::with_capacity(MAX_LOGS),
            log_rx,
            custom_msg_tx,
            should_quit: false,
            input_channel: "testchannel".to_string(),
            input_username: default_username.clone(),
            input_display_name: titlecase(&default_username), // Initialize based on username
            input_color: "#FFFFFF".to_string(),
            input_message: String::new(),
            current_focus: InputFocus::Channel,
            log_list_state: ListState::default(),
        }
    }

    pub async fn run_ui(&mut self, mut terminal: Terminal<impl Backend>) -> Result<()> {
        if !self.logs.is_empty() {
            self.log_list_state.select(Some(self.logs.len() - 1));
        }

        while !self.should_quit {
            let mut new_log_added = false;
            while let Ok(log_entry) = self.log_rx.try_recv() {
                let log_str = match log_entry {
                    ServerLog::Incoming(addr, msg) => format!("[{}] C: {}", addr, msg),
                    ServerLog::Outgoing(addr, msg) => format!("[{}] S: {}", addr, msg),
                    ServerLog::Internal(msg) => format!("[SERVER] {}", msg),
                    ServerLog::ClientConnected(addr) => {
                        format!("[SERVER] Client connected: {}", addr)
                    }
                    ServerLog::ClientDisconnected(addr) => {
                        format!("[SERVER] Client disconnected: {}", addr)
                    }
                };
                if self.logs.len() >= MAX_LOGS {
                    self.logs.pop_front();
                    if let Some(selected) = self.log_list_state.selected() {
                        if selected > 0 {
                            self.log_list_state.select(Some(selected - 1));
                        } else if !self.logs.is_empty() {
                            self.log_list_state.select(Some(0));
                        } else {
                            self.log_list_state.select(None);
                        }
                    }
                }
                self.logs.push_back(log_str);
                new_log_added = true;
            }

            if new_log_added && !self.logs.is_empty() {
                self.log_list_state.select(Some(self.logs.len() - 1));
            }

            terminal.draw(|f| self.draw_ui(f))?;

            if event::poll(std::time::Duration::from_millis(50))? {
                if let CrosstermEvent::Key(key_event) = event::read()? {
                    self.handle_key_event(key_event).await;
                }
            }
        }
        Ok(())
    }

    async fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Press {
            match key_event.code {
                KeyCode::Esc => self.should_quit = true,
                KeyCode::Tab => self.current_focus = self.current_focus.next(),
                KeyCode::BackTab => self.current_focus = self.current_focus.prev(),
                KeyCode::PageUp => {
                    if let Some(selected) = self.log_list_state.selected() {
                        self.log_list_state
                            .select(Some(selected.saturating_sub(5.min(selected))));
                    } else if !self.logs.is_empty() {
                        self.log_list_state.select(Some(0));
                    }
                }
                KeyCode::PageDown => {
                    if let Some(selected) = self.log_list_state.selected() {
                        if !self.logs.is_empty() {
                            self.log_list_state
                                .select(Some((selected + 5).min(self.logs.len() - 1)));
                        }
                    } else if !self.logs.is_empty() {
                        self.log_list_state.select(Some(self.logs.len() - 1));
                    }
                }
                KeyCode::Char(c) => {
                    let old_username_val = self.input_username.clone(); // Capture before change
                    match self.current_focus {
                        InputFocus::Channel => self.input_channel.push(c),
                        InputFocus::Username => self.input_username.push(c),
                        InputFocus::DisplayName => self.input_display_name.push(c),
                        InputFocus::Color => self.input_color.push(c),
                        InputFocus::Message => self.input_message.push(c),
                    }
                    // Auto-update display name if username was changed AND display name was tied to it or empty
                    if matches!(self.current_focus, InputFocus::Username) {
                        if self.input_display_name.is_empty()
                            || self.input_display_name == titlecase(&old_username_val)
                        {
                            self.input_display_name = titlecase(&self.input_username);
                        }
                    }
                }
                KeyCode::Backspace => {
                    let old_username_val = self.input_username.clone(); // Capture before change
                    match self.current_focus {
                        InputFocus::Channel => {
                            self.input_channel.pop();
                        }
                        InputFocus::Username => {
                            self.input_username.pop();
                        }
                        InputFocus::DisplayName => {
                            self.input_display_name.pop();
                        }
                        InputFocus::Color => {
                            self.input_color.pop();
                        }
                        InputFocus::Message => {
                            self.input_message.pop();
                        }
                    }
                    // Auto-update display name if username was changed AND display name was tied to it
                    if matches!(self.current_focus, InputFocus::Username) {
                        if self.input_display_name == titlecase(&old_username_val) {
                            self.input_display_name = titlecase(&self.input_username);
                        }
                        // If username becomes empty and display name was its titlecase, clear display name too
                        if self.input_username.is_empty()
                            && self.input_display_name == titlecase(&old_username_val)
                        {
                            self.input_display_name.clear();
                        }
                    }
                }
                KeyCode::Enter => {
                    if let InputFocus::Message = self.current_focus {
                        if !self.input_channel.is_empty()
                            && !self.input_username.is_empty()
                            // Display name can now be explicitly set, or fall back to titlecased username
                            && !self.input_message.is_empty()
                        {
                            let display_name_to_send = if self.input_display_name.is_empty() {
                                titlecase(&self.input_username)
                            } else {
                                self.input_display_name.clone()
                            };

                            let color_to_send = if self.input_color.is_empty()
                                || !self.input_color.starts_with('#')
                            {
                                "#FFFFFF".to_string() // Default to white if empty or invalid format
                            } else {
                                self.input_color.clone()
                            };

                            let msg = CustomMessage {
                                channel: self.input_channel.clone(),
                                username: self.input_username.clone(),
                                display_name: display_name_to_send,
                                message: self.input_message.clone(),
                                color: color_to_send,
                            };
                            if self.custom_msg_tx.send(msg).await.is_err() {
                                if self.logs.len() >= MAX_LOGS {
                                    self.logs.pop_front();
                                }
                                self.logs
                                    .push_back("Error: Failed to send custom message.".to_string());
                            }
                            self.input_message.clear();
                        }
                    } else {
                        self.current_focus = self.current_focus.next();
                    }
                }
                _ => {}
            }
        }
    }

    fn draw_ui(&mut self, f: &mut Frame) {
        let main_layout =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(f.area());

        self.draw_logs(f, main_layout[0]);
        self.draw_input_panel(f, main_layout[1]);
    }

    fn draw_logs(&mut self, f: &mut Frame, area: Rect) {
        let log_items: Vec<ListItem> = self
            .logs
            .iter()
            .map(|log| ListItem::new(Text::raw(log.clone())))
            .collect();
        let list = List::new(log_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("IRC Server Logs"),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
            .highlight_symbol(">> ");
        f.render_stateful_widget(list, area, &mut self.log_list_state);
    }

    fn draw_input_panel(&self, f: &mut Frame, area: Rect) {
        let input_layout = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

        let channel_input = Paragraph::new(self.input_channel.as_str())
            .style(if matches!(self.current_focus, InputFocus::Channel) {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Channel (no #)"),
            );
        f.render_widget(channel_input, input_layout[0]);

        let username_input = Paragraph::new(self.input_username.as_str())
            .style(if matches!(self.current_focus, InputFocus::Username) {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Username (login)"),
            );
        f.render_widget(username_input, input_layout[1]);

        let display_name_input = Paragraph::new(self.input_display_name.as_str())
            .style(if matches!(self.current_focus, InputFocus::DisplayName) {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            })
            .block(Block::default().borders(Borders::ALL).title("Display Name"));
        f.render_widget(display_name_input, input_layout[2]);

        let color_input = Paragraph::new(self.input_color.as_str())
            .style(if matches!(self.current_focus, InputFocus::Color) {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Color (e.g. #FF00FF)"),
            );
        f.render_widget(color_input, input_layout[3]);

        let message_input = Paragraph::new(self.input_message.as_str())
            .style(if matches!(self.current_focus, InputFocus::Message) {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Message (Enter to send)"),
            );
        f.render_widget(message_input, input_layout[4]);

        match self.current_focus {
            InputFocus::Channel => f.set_cursor_position(Position::new(
                input_layout[0].x + 1 + self.input_channel.chars().count() as u16,
                input_layout[0].y + 1,
            )),
            InputFocus::Username => f.set_cursor_position(Position::new(
                input_layout[1].x + 1 + self.input_username.chars().count() as u16,
                input_layout[1].y + 1,
            )),
            InputFocus::DisplayName => f.set_cursor_position(Position::new(
                input_layout[2].x + 1 + self.input_display_name.chars().count() as u16,
                input_layout[2].y + 1,
            )),
            InputFocus::Color => f.set_cursor_position(Position::new(
                input_layout[3].x + 1 + self.input_color.chars().count() as u16,
                input_layout[3].y + 1,
            )),
            InputFocus::Message => f.set_cursor_position(Position::new(
                input_layout[4].x + 1 + self.input_message.chars().count() as u16,
                input_layout[4].y + 1,
            )),
        }
    }
}

fn titlecase(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut c = s.chars();
    c.next()
        .unwrap_or_default()
        .to_uppercase()
        .collect::<String>()
        + c.as_str()
}
