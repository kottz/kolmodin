// src/main.rs

mod irc_server;
mod ui;

use color_eyre::Result;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    // Channels for communication
    // Server -> UI (for logs)
    let (log_tx, log_rx) = mpsc::channel::<irc_server::ServerLog>(100);
    // UI -> Server (for sending custom messages)
    let (custom_msg_tx, custom_msg_rx) = mpsc::channel::<irc_server::CustomMessage>(32);

    // --- Setup TUI ---
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?; // Clear screen before first draw

    // --- Spawn IRC Server Task ---
    let server_log_tx_clone = log_tx.clone(); // Clone for the server task
    tokio::spawn(async move {
        if let Err(e) = irc_server::run_server(server_log_tx_clone, custom_msg_rx).await {
            // Log server error (e.g., using log_tx if UI is still running, or eprintln)
            // For simplicity, sending to log_tx, assuming UI might catch it.
            let _ = log_tx
                .send(irc_server::ServerLog::Internal(format!(
                    "IRC Server exited with error: {}",
                    e
                )))
                .await;
            // Or eprintln!("IRC Server exited with error: {}", e);
        }
    });

    // --- Run UI (in the main thread/task) ---
    let mut app_ui = ui::App::new(log_rx, custom_msg_tx);
    let ui_result = app_ui.run_ui(terminal).await;

    // --- Restore Terminal ---
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    // terminal.show_cursor()?; // Not strictly needed if `LeaveAlternateScreen` handles it

    // Handle UI result
    if let Err(e) = ui_result {
        eprintln!("UI exited with error: {}", e);
        return Err(e.into());
    }

    Ok(())
}
