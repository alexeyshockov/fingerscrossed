use crate::config::TrxConfig;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct LogLine {
    trace_id: String,
    received_at: u128,
    line: String,
    json_line: Value,
}

#[derive(Debug)]
struct LogLineParser {
    trx_id_field: String,
}

impl LogLineParser {
    fn parse(&self, line: &str) -> Result<LogLine> {
        let json_line = serde_json::from_str::<Value>(line)?;
        let trace_id = json_line
            .get(&self.trx_id_field)
            .and_then(|v| v.as_str())
            .ok_or(anyhow!("Trace ID cannot be extracted"))?;
        let received_at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_millis();
        Ok(LogLine {
            trace_id: trace_id.to_string(),
            line: line.to_string(),
            received_at,
            json_line,
        })
    }
}

#[derive(Debug)]
struct Transaction {
    triggered: bool,
    log_lines: Vec<LogLine>,
}

impl Transaction {
    fn new() -> Self {
        Self {
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

#[derive(Debug)]
pub enum Message {
    /// A new log line to process
    Line(String),
    /// Time to clean up expired transactions
    Cleanup(u128),
    /// Stop the processing loop
    Shutdown,
}

struct TrxStore {
    buffers: HashMap<String, Transaction>,
    trx_timeout: u128,
}

impl TrxStore {
    fn get(&mut self, line: &LogLine) -> &mut Transaction {
        let tid = &line.trace_id;
        self.buffers.entry(tid.clone()).or_insert_with(Transaction::new)
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

    fn handle_trx_line(&mut self, line: &str) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_line_parser() {
        let parser = LogLineParser {
            trx_id_field: "trace_id".to_string(),
        };

        let line = r#"{"trace_id": "123", "message": "test"}"#;
        let result = parser.parse(line);
        assert!(result.is_ok());

        let log_line = result.unwrap();
        assert_eq!(log_line.trace_id, "123");
    }

    #[test]
    fn test_transaction_age() {
        let transaction = Transaction::new();
        let now = 1000;
        assert_eq!(transaction.age(now), 0);
    }

    #[test]
    fn test_transaction_add_line() {
        let line = serde_json::json!({"trace_id": "123", "message": "test"});
        let mut transaction = Transaction::new();
        let log_line = LogLine {
            trace_id: "123".to_string(),
            received_at: 1000,
            line: line.to_string(),
            json_line: line,
        };
        transaction.add(log_line.clone());
        assert_eq!(transaction.log_lines.len(), 1);
        assert_eq!(transaction.log_lines[0].trace_id, "123");
    }
}
