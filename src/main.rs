mod trx;

use trx::*;

use clap::Parser;
use std::io::{self, BufRead, BufReader, IsTerminal};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, SystemTime};

#[derive(Parser)]
struct Cli {
    /// Command to run as a subprocess
    cmd: Option<String>,
    /// Arguments for the subprocess
    args: Vec<String>,
    /// Trace ID JSON field
    #[clap(long, default_value = "trace_id")]
    id_field: String,
}

fn handle_target_app(ch: Sender<Message>, cmd: &str, args: Vec<String>) {
    let mut command = Command::new(cmd);
    command.args(&args);
    command.stdout(Stdio::piped());

    let mut child = command.spawn().expect("Failed to start the target app");
    let stdout = child.stdout.take().expect("Failed to capture target's stdout");

    // The target app will receive the same SIGINT/SIGTERM as we are, because of the same process
    // group. So no need to kill it explicitly, just read the output stream until EOF.

    handle_lines(ch, BufReader::new(stdout));
}

fn handle_lines<R: BufRead>(ch: Sender<Message>, lines: R) {
    for line in lines.lines() {
        match line {
            Ok(line) => {
                ch.send(Message::Line(line)).expect("Cannot process a line");
            }
            Err(e) => {
                eprintln!("Error reading from the target app: {}", e);
                break;
            }
        }
    }
    ch.send(Message::Shutdown).expect("Error shutting down");
}

fn main() {
    let args = Cli::parse();

    let transaction_timeout = 5000; // Milliseconds
    let cleanup_interval = Duration::from_secs(1);

    let stdin = io::stdin();
    let is_terminal = stdin.is_terminal();
    let (sender, receiver) = channel();

    let mut line_handler = TrxHandler::new(&args.id_field, transaction_timeout);

    {
        let sender = sender.clone();
        if let Some(cmd) = args.cmd {
            std::thread::spawn(move || handle_target_app(sender, &cmd, args.args));
        } else {
            std::thread::spawn(move || handle_lines(sender, stdin.lock()));
        }
    }
    {
        let sender = sender.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(cleanup_interval);
                let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis();
                sender.send(Message::Cleanup(now)).unwrap();
            }
        });
    }

    if is_terminal {
        ctrlc::set_handler(move || {
            sender.send(Message::Shutdown).expect("Error shutting down");
        }).expect("Error setting Ctrl-C handler");
    }

    line_handler.observe(receiver)
}
