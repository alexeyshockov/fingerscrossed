use serde_json::Value;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::SystemTime;

pub struct LogLine {
    trace_id: String,
    level: String, // error, warn, info, debug, trace
    line: String,
    received_at: u128,
}

struct LogLineParser {
    trace_id_field: String,
    level_field: String,
}

impl LogLineParser {
    fn parse(&self, line: &str) -> Result<LogLine, &'static str> {
        let json = serde_json::from_str::<Value>(line).map_err(|_| "Invalid JSON")?;
        let trace_id = json
            .get(&self.trace_id_field)
            .and_then(|v| v.as_str())
            .ok_or("TraceId field not found")?;
        let level = json
            .get(&self.level_field)
            .and_then(|v| v.as_str())
            .ok_or("Level field not found")?;
        let received_at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis();
        Ok(LogLine {
            trace_id: trace_id.to_string(),
            level: level.to_string(),
            line: line.to_string(),
            received_at,
        })
    }
}

struct Transaction {
    trace_id: String,
    triggered: bool,
    log_lines: Vec<LogLine>,
}

impl Transaction {
    fn new(trace_id: String) -> Self {
        Self {
            trace_id,
            triggered: false,
            log_lines: Vec::with_capacity(10),
        }
    }

    fn add(&mut self, log_line: LogLine) {
        self.log_lines.push(log_line);
    }

    fn age(&self, now: u128) -> u128 {
        self.log_lines.last().map(|l| if now > l.received_at { now - l.received_at } else { 0 }).unwrap_or(0)
    }
}

fn triggers(line: &LogLine) -> bool {
    // Make configurable, later
    line.level.to_ascii_lowercase() == "error"
}

fn ends(_: &Transaction, _: &LogLine) -> bool {
    // TODO Make configurable
    false
}

pub enum Message {
    Line(String),
    Cleanup(u128),
}


// fn get_trx(mut store: TrxStore, line: &LogLine) -> &mut Transaction {
//     store.entry(line.trace_id.clone()).or_insert_with(|| Transaction::new(&line))
// }

struct TrxStore {
    buffers: HashMap<String, Transaction>,
    transaction_timeout: u128,
}

impl TrxStore {
    fn get(&mut self, line: &LogLine) -> &mut Transaction {
        let tid = &line.trace_id;
        self.buffers.entry(tid.clone()).or_insert_with(|| Transaction::new(tid.clone()))
    }

    fn remove(&mut self, trace_id: &str) {
        self.buffers.remove(trace_id);
    }

    fn cleanup(&mut self, now: u128) {
        let mut to_remove = Vec::new();
        for (trace_id, transaction) in &self.buffers {
            if transaction.age(now) > self.transaction_timeout {
                to_remove.push(trace_id.clone());
            }
        }
        for trace_id in to_remove {
            self.buffers.remove(&trace_id);
        }
    }
}

pub struct TrxHandler {
    store: TrxStore,
    line_parser: LogLineParser,
}

impl TrxHandler {
    pub fn new(trace_id_field: &str, level_field: &str, transaction_timeout: u128) -> Self {
        Self {
            store: TrxStore {
                buffers: HashMap::new(),
                transaction_timeout,
            },
            line_parser: LogLineParser {
                trace_id_field: trace_id_field.to_string(),
                level_field: level_field.to_string(),
            },
        }
    }

    fn handle_trx_line(&mut self, line: &str) -> Result<(), &'static str> {
        let log_line = self.line_parser.parse(line)?;
        let trx = self.store.get(&log_line);
        if trx.triggered {
            println!("{}", line);
            return Ok(());
        }
        let trace_id = log_line.trace_id.clone();
        let does_end = ends(&trx, &log_line);
        eprintln!("Log level: {}, triggers: {}", log_line.level, triggers(&log_line));
        if triggers(&log_line) {
            trx.triggered = true;
            // And flush the buffer
            for trx_line in &trx.log_lines {
                println!("{}", trx_line.line);
            }
            // And the current line
            println!("{}", line);
        } else {
            trx.add(log_line);
        }
        if does_end {
            self.store.remove(&trace_id);
        }
        Ok(())
    }

    pub fn observe(&mut self, ch: Receiver<Message>) {
        loop {
            match ch.recv() {
                Ok(Message::Line(line)) => {
                    if let Err(_) = self.handle_trx_line(&line) {
                        println!("{}", line);
                    }
                },
                Ok(Message::Cleanup(now)) => {
                    self.store.cleanup(now);
                },
                Err(_) => {
                    // No more senders, exit
                }
            }
        }
    }
}
