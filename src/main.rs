mod trx;

use trx::*;

use clap::Parser;
use std::io::{self, BufRead, IsTerminal, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

#[derive(Parser)]
struct Cli {
    /// Command to run as a subprocess
    cmd: Option<String>,
    /// Arguments for the subprocess
    args: Vec<String>,
}

fn handle_target_app(ch: Sender<Message>, cmd: &str, args: Vec<String>) {
    let mut command = Command::new(cmd);
    command.args(&args);
    command.stdout(Stdio::piped());

    let mut child = command.spawn().expect("Failed to start the target app");
    let stdout = child.stdout.take().expect("Failed to capture target's stdout");

    // The target app will receive the same SIGINT/SIGTERM as we are, because of the same process
    // group. So no need to kill it explicitly, just read the output stream until EOF.

    for line in io::BufReader::new(stdout).lines() {
        if let Ok(line) = line {
            ch.send(Message::Line(line)).expect("Cannot process a line");
        }
    }
}

fn handle_stdin(state: &AppState, ch: Sender<Message>) {
    let pipe_mode = !state.is_terminal;
    for line in io::stdin().lock().lines() {
        if !pipe_mode && state.shutting_down.load(Relaxed) {
            break;
        }
        if let Ok(line) = line {
            eprintln!("Received: {}", line);
            ch.send(Message::Line(line)).expect("Cannot process a line");
        }
    }
}

fn spawn_cleanup_thread(state: &AppState, ch: Sender<Message>, interval: Duration) -> JoinHandle<()> {
    let state = state.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(interval);
            if state.shutting_down.load(Relaxed) {
                break;
            }
            let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis();
            ch.send(Message::Cleanup(now)).unwrap();
        }
    })
}

#[derive(Clone)]
struct AppState {
    shutting_down: Arc<AtomicBool>,
    is_terminal: bool,
}

impl AppState {
    fn new() -> Self {
        let app_state = Self {
            shutting_down: Arc::new(AtomicBool::new(false)),
            is_terminal: io::stdin().is_terminal(),
        };

        if app_state.is_terminal {
            let shutting_down = app_state.shutting_down.clone();
            ctrlc::set_handler(move || {
                print!("Shutting down, press Enter to exit");
                io::stdout().flush().unwrap();
                shutting_down.store(true, Relaxed);
            }).expect("Error setting Ctrl-C handler");
        }

        app_state
    }
}

fn main() {
    let args = Cli::parse();

    let trace_id_field = "trace_id";
    let level_field = "level";
    let transaction_timeout = 5000; // Milliseconds
    let cleanup_interval = Duration::from_secs(1);

    let (sender, receiver) = channel();

    let app_state = AppState::new();

    let mut line_handler = TrxHandler::new(trace_id_field, level_field, transaction_timeout);

    let handler_thread = std::thread::spawn(move || {
        line_handler.observe(receiver)
    });
    let cleanup_thread = spawn_cleanup_thread(&app_state, sender.clone(), cleanup_interval);

    if let Some(cmd) = args.cmd {
        handle_target_app(sender, &cmd, args.args)
    } else {
        handle_stdin(&app_state, sender)
    }

    handler_thread.join().unwrap();
}