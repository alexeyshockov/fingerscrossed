use regex::RegexBuilder;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

pub struct TrxConfig {
    pub id_field: String,
    pub cleanup_interval: Duration,
    pub timeout: u64, // Milliseconds, since the last log line of the transaction
    flush_triggers: Vec<Trigger>,
    completion_triggers: Vec<Trigger>,
}

impl TrxConfig {
    pub fn default() -> Self {
        Self {
            id_field: "trace_id".to_string(),
            cleanup_interval: Duration::from_millis(1000),
            timeout: 5000,
            flush_triggers: vec![
                HashMap::from([
                    ("level".to_string(), FieldMatcher::StrOneOf(vec![
                        "error".to_string(),
                        "fatal".to_string(),
                        "critical".to_string(),
                    ])),
                ]),
            ],
            completion_triggers: Vec::new(),
        }
    }

    pub fn from(conf: Config) -> Self {
        Self {
            id_field: conf.id_field.unwrap_or("trace_id".to_string()),
            cleanup_interval: Duration::from_millis(conf.cleanup_interval.unwrap_or(1000)),
            timeout: conf.timeout.unwrap_or(5000),
            flush_triggers: conf.flush_triggers,
            completion_triggers: conf.completion_triggers.unwrap_or(Vec::new()),
        }
    }

    pub fn matches(&self, line: &Value) -> (bool, bool) {
        let flushes = matches(line, &self.flush_triggers);
        let completes = matches(line, &self.completion_triggers);
        (flushes, completes)
    }
}

#[derive(Deserialize)]
pub struct Config {
    id_field: Option<String>,
    cleanup_interval: Option<u64>, // Milliseconds
    timeout: Option<u64>, // Milliseconds, since the last log line of the transaction
    flush_triggers: Vec<Trigger>,
    completion_triggers: Option<Vec<Trigger>>,
}

type Trigger = HashMap<String, FieldMatcher>;

fn matches(line: &Value, triggers: &Vec<Trigger>) -> bool {
    for trigger in triggers {
        for (field, matcher) in trigger {
            let field_value = line.get(field);
            if let Some(value) = field_value {
                if matcher.matches(value) {
                    return true;
                }
            }
        }
    }
    false
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FieldMatcher {
    StrVal(String),
    IntVal(i64),
    StrOneOf(Vec<String>),
    IntOneOf(Vec<i64>),
    Regex { regex: String },
}

impl FieldMatcher {
    // TODO Freeze the regex
    fn matches(&self, value: &Value) -> bool {
        match self {
            FieldMatcher::StrVal(expected) => {
                value.as_str().map_or(false, |v| v.eq_ignore_ascii_case(expected))
            }
            FieldMatcher::IntVal(expected) => {
                value.as_i64().map_or(false, |v| v == *expected)
            }
            FieldMatcher::StrOneOf(expected) => {
                value.as_str().map_or(false, |v| expected.iter().any(|e| e.eq_ignore_ascii_case(v)))
            }
            FieldMatcher::IntOneOf(expected) => {
                value.as_i64().map_or(false, |v| expected.iter().any(|e| e == &v))
            }
            FieldMatcher::Regex { regex } => {
                let re = RegexBuilder::new(regex).case_insensitive(true).build().unwrap();
                value.as_str().map_or(false, |v| re.is_match(v))
            }
        }
    }
}