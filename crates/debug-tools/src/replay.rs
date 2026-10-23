use crate::input::Command;
use serde_json::Value;

/// Adapters translate these operations to the application's ordinary input API.
pub enum Action {
    Advance,
    Complete,
    Choose(String),
    Restart,
}

/// Read-only inspection and ordinary input; privileged state mutation is separate.
pub trait DebugTarget {
    fn busy(&self) -> bool;
    fn tick(&mut self);
    fn apply(&mut self, action: Action) -> Result<(), String>;
    fn choices(&self) -> Vec<&str>;
    fn snapshot(&self) -> Value;
    fn location(&self) -> String;
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Updated,
    Checked,
    Capture(String),
    Quit,
}

/// One shared budget bounds commands, actions and virtual presentation frames.
pub struct Runner<T> {
    target: T,
    remaining: usize,
}

impl<T: DebugTarget> Runner<T> {
    pub fn new(target: impl Into<T>, limit: usize) -> Self {
        Self {
            target: target.into(),
            remaining: limit,
        }
    }
    pub fn target(&self) -> &T {
        &self.target
    }
    fn step(&mut self) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or("execution limit reached")?;
        Ok(())
    }
    pub fn settle(&mut self) -> Result<(), String> {
        while self.target.busy() {
            self.step()?;
            self.target.tick();
        }
        Ok(())
    }
    fn action(&mut self, action: Action) -> Result<(), String> {
        self.step()?;
        self.target.apply(action)?;
        self.settle()
    }
    pub fn snapshot(&self) -> Value {
        self.target.snapshot()
    }
    pub fn location(&self) -> String {
        self.target.location()
    }
    pub fn execute(&mut self, command: &Command) -> Result<Outcome, String> {
        self.step()?;
        self.settle()?;
        match command {
            Command::Advance { count } => {
                for _ in 0..*count {
                    self.action(Action::Advance)?;
                }
            }
            Command::Complete => self.action(Action::Complete)?,
            Command::Choose { value, numbered } => {
                let choices = self.target.choices();
                let id = if *numbered && value.parse::<usize>().is_ok() {
                    let number = value.parse::<usize>().map_err(|e| e.to_string())?;
                    *choices
                        .get(number.checked_sub(1).ok_or("choice numbers start at 1")?)
                        .ok_or("choice number out of range")?
                } else {
                    *choices
                        .iter()
                        .find(|c| **c == value.as_str())
                        .ok_or("choice ID is not displayed")?
                };
                self.action(Action::Choose(id.to_owned()))?;
            }
            Command::Restart => self.action(Action::Restart)?,
            Command::Check { pointer, expected } => {
                let snapshot = self.snapshot();
                let actual = snapshot
                    .pointer(pointer)
                    .ok_or_else(|| format!("missing state: {pointer}"))?;
                if actual != expected {
                    return Err(format!(
                        "check {pointer}: expected {expected}, actual {actual}"
                    ));
                }
                return Ok(Outcome::Checked);
            }
            Command::Until { pointer, expected } => loop {
                if self.snapshot().pointer(pointer) == Some(expected) {
                    break;
                }
                self.action(Action::Advance)
                    .map_err(|e| format!("cannot reach {pointer} = {expected}: {e}"))?;
            },
            Command::Capture { name } => return Ok(Outcome::Capture(name.clone())),
            Command::Quit => return Ok(Outcome::Quit),
            Command::Show => {}
        }
        Ok(Outcome::Updated)
    }
}
