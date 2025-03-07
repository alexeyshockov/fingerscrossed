use crate::config::{TrxConfig};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::SystemTime;

pub struct LogLine {
    trace_id: String,
    received_at: u128,
    line: String,
    json_line: Value,
}

struct LogLineParser {
    trx_id_field: String,
}

impl LogLineParser {
    fn parse(&self, line: &str) -> Result<LogLine, &'static str> {
        let json_line = serde_json::from_str::<Value>(line).map_err(|_| "Invalid JSON line")?;
        let trace_id = json_line
            .get(&self.trx_id_field)
            .and_then(|v| v.as_str())
            .ok_or("Trace ID cannot be extracted")?;
        let received_at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis();
        Ok(LogLine {
            trace_id: trace_id.to_string(),
            line: line.to_string(),
            received_at,
            json_line,
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

pub enum Message {
    Line(String),
    Cleanup(u128),
    Shutdown,
}

struct TrxStore {
    buffers: HashMap<String, Transaction>,
    trx_timeout: u128, // TODO Move to Transaction
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
        let mut to_remove = Vec::with_capacity(self.buffers.len());
        for (trace_id, transaction) in &self.buffers {
            if transaction.age(now) > self.trx_timeout {
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
    conf: TrxConfig,
}

impl TrxHandler {
    pub fn new(conf: TrxConfig) -> Self {
        Self {
            store: TrxStore {
                buffers: HashMap::new(),
                trx_timeout: conf.timeout as u128,
            },
            line_parser: LogLineParser {
                trx_id_field: conf.id_field.clone(),
            },
            conf,
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
        let (triggers_flush, completes_trx) = self.conf.matches(&log_line.json_line);
        if triggers_flush {
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
        if completes_trx {
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
                Ok(Message::Shutdown) | Err(_) => {
                    break;
                },
            }
        }
    }
}