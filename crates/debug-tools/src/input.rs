//! Text commands shared by interactive execution, command files and browser replay.
use serde::Serialize;
use serde_json::Value;
use std::io::BufRead;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum Command {
    Advance { count: usize },
    Complete,
    Choose { value: String, numbered: bool },
    Show,
    Check { pointer: String, expected: Value },
    Until { pointer: String, expected: Value },
    Restart,
    Capture { name: String },
    Quit,
}
#[derive(Debug, Serialize)]
pub struct InputLine {
    pub line: usize,
    pub source: String,
    pub command: Command,
}
pub struct CliInput<R> {
    reader: R,
    line: usize,
}
impl<R: BufRead> CliInput<R> {
    pub fn new(reader: R) -> Self {
        Self { reader, line: 0 }
    }
    pub fn next_command(&mut self) -> Result<Option<InputLine>, String> {
        loop {
            let mut source = String::new();
            let count = self
                .reader
                .read_line(&mut source)
                .map_err(|e| format!("line {}: {e}", self.line + 1))?;
            if count == 0 {
                return Ok(None);
            }
            self.line += 1;
            let source = source.trim().to_owned();
            if let Some(command) =
                Self::parse(&source).map_err(|e| format!("line {} [{source}]: {e}", self.line))?
            {
                return Ok(Some(InputLine {
                    line: self.line,
                    source,
                    command,
                }));
            }
        }
    }
    pub fn parse(source: &str) -> Result<Option<Command>, String> {
        let source = source.trim_start_matches('\u{feff}').trim();
        if source.is_empty() || source.starts_with('#') {
            return Ok(None);
        }
        let (op, arg) = source
            .split_once(char::is_whitespace)
            .unwrap_or((source, ""));
        let arg = arg.trim();
        let no_args = |command| {
            if arg.is_empty() {
                Ok(Some(command))
            } else {
                Err("unexpected arguments".into())
            }
        };
        match op {
            "advance" => {
                let count = if arg.is_empty() {
                    1
                } else {
                    arg.parse::<usize>()
                        .map_err(|_| "advance requires a positive count")?
                };
                if count == 0 {
                    return Err("advance requires a positive count".into());
                }
                Ok(Some(Command::Advance { count }))
            }
            "complete" => no_args(Command::Complete),
            "show" => no_args(Command::Show),
            "restart" => no_args(Command::Restart),
            "quit" => no_args(Command::Quit),
            "choose" | "choose-id" if !arg.is_empty() && !arg.contains(char::is_whitespace) => {
                Ok(Some(Command::Choose {
                    value: arg.into(),
                    numbered: op == "choose",
                }))
            }
            "check" | "until" => {
                let (pointer, expected) = arg
                    .split_once(char::is_whitespace)
                    .ok_or("expected JSON pointer and JSON value")?;
                if !pointer.starts_with('/') {
                    return Err("expected JSON pointer starting with /".into());
                }
                let expected = serde_json::from_str(expected)
                    .map_err(|e| format!("invalid expected JSON: {e}"))?;
                Ok(Some(if op == "check" {
                    Command::Check {
                        pointer: pointer.into(),
                        expected,
                    }
                } else {
                    Command::Until {
                        pointer: pointer.into(),
                        expected,
                    }
                }))
            }
            "capture"
                if !arg.is_empty()
                    && arg
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') =>
            {
                Ok(Some(Command::Capture { name: arg.into() }))
            }
            _ if arg.is_empty() && op.parse::<usize>().is_ok() => Ok(Some(Command::Choose {
                value: op.into(),
                numbered: true,
            })),
            _ => Err("unknown command or invalid arguments".into()),
        }
    }
}
