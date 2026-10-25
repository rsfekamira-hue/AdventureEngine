//! Shared command execution, diagnostics and quiet output policy.
use crate::{
    input::{CliInput, Command},
    replay::{DebugTarget, Outcome, Runner},
};
use std::io::{BufRead, Write};

/// Application-specific text presentation and optional capture adapter.
pub trait CliHost<T: DebugTarget> {
    fn display(&mut self, target: &T, output: &mut dyn Write) -> Result<(), String>;
    fn capture(&mut self, target: &T, name: &str) -> Result<String, String>;
}

pub fn run<T: DebugTarget, R: BufRead>(
    runner: &mut Runner<T>,
    input: &mut CliInput<R>,
    host: &mut impl CliHost<T>,
    output: &mut impl Write,
    quiet: bool,
) -> Result<(), String> {
    runner
        .settle()
        .map_err(|e| format!("{}: {e}", runner.location()))?;
    if !quiet {
        host.display(runner.target(), output)?;
    }
    while let Some(line) = input
        .next_command()
        .map_err(|e| format!("{}: {e}", runner.location()))?
    {
        let outcome = runner.execute(&line.command).map_err(|e| {
            format!(
                "line {} [{}] {}: {e}",
                line.line,
                line.source,
                runner.location()
            )
        })?;
        let result = match outcome {
            Outcome::Quit => break,
            Outcome::Checked => writeln!(output, "OK line {} [{}]", line.line, line.source)
                .map_err(|e| e.to_string()),
            Outcome::Capture(name) => host.capture(runner.target(), &name).and_then(|message| {
                writeln!(output, "CAPTURE {message}").map_err(|e| e.to_string())
            }),
            Outcome::Updated if matches!(line.command, Command::Show) => {
                writeln!(output, "{}", runner.snapshot()).map_err(|e| e.to_string())
            }
            Outcome::Updated if quiet => Ok(()),
            Outcome::Updated => host.display(runner.target(), output),
        };
        result.map_err(|e| {
            format!(
                "line {} [{}] {}: {e}",
                line.line,
                line.source,
                runner.location()
            )
        })?;
    }
    Ok(())
}
