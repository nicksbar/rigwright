//! Shared, portable reports emitted by the hardware-validation examples.

use std::fmt::{Debug, Display};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct ProbeLog {
    tool: String,
    model: String,
    port: String,
    baud: u32,
    started_unix_ms: u128,
    records: Vec<ProbeRecord>,
    metrics: Option<String>,
}

#[derive(Debug)]
struct ProbeRecord {
    name: String,
    status: &'static str,
    detail: String,
}

impl ProbeLog {
    pub fn new(
        tool: impl Into<String>,
        model: impl Into<String>,
        port: impl Into<String>,
        baud: u32,
    ) -> Self {
        Self {
            tool: tool.into(),
            model: model.into(),
            port: port.into(),
            baud,
            started_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_millis()),
            records: Vec::new(),
            metrics: None,
        }
    }

    pub fn pass(&mut self, name: impl Into<String>, detail: impl Display) {
        self.record(name, "pass", detail);
    }

    pub fn fail(&mut self, name: impl Into<String>, detail: impl Display) {
        self.record(name, "fail", detail);
    }

    pub fn skip(&mut self, name: impl Into<String>, detail: impl Display) {
        self.record(name, "skip", detail);
    }

    pub fn set_metrics(&mut self, metrics: impl Debug) {
        self.metrics = Some(format!("{metrics:?}"));
    }

    pub fn write(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let records = self
            .records
            .iter()
            .map(|record| {
                format!(
                    "{{\"name\":\"{}\",\"status\":\"{}\",\"detail\":\"{}\"}}",
                    json_escape(&record.name),
                    record.status,
                    json_escape(&record.detail)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let metrics = self.metrics.as_deref().map_or_else(
            || "null".to_owned(),
            |value| format!("\"{}\"", json_escape(value)),
        );
        let document = format!(
            "{{\"tool\":\"{}\",\"model\":\"{}\",\"port\":\"{}\",\"baud\":{},\"started_unix_ms\":{},\"records\":[{}],\"transport_metrics\":{}}}\n",
            json_escape(&self.tool),
            json_escape(&self.model),
            json_escape(&self.port),
            self.baud,
            self.started_unix_ms,
            records,
            metrics
        );
        fs::write(path, document)
    }

    fn record(&mut self, name: impl Into<String>, status: &'static str, detail: impl Display) {
        self.records.push(ProbeRecord {
            name: name.into(),
            status,
            detail: detail.to_string(),
        });
    }
}

fn json_escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            character if character.is_control() => {
                format!("\\u{:04x}", character as u32).chars().collect()
            }
            character => vec![character],
        })
        .collect()
}
