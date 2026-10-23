//! Standalone inspection of Core definitions. No resource packs or game dependency.
use engine_core::event_system::{EntryMode, EventInput, EventSystem, JsonHolder};
use engine_debug_tools::{
    input::{CliInput, Command},
    replay::{Action, DebugTarget, Outcome, Runner},
};
use std::io::{self, BufRead, BufReader};

struct CoreTarget {
    system: EventSystem,
    entry: String,
}
impl CoreTarget {
    fn start(&mut self) -> Result<(), String> {
        let event = self
            .system
            .event_id(&self.entry)
            .ok_or("unknown entry event")?;
        if !self.system.update(EventInput::Start {
            event,
            mode: EntryMode::Start,
        }) {
            return Err("start rejected".into());
        }
        Ok(())
    }
}
impl DebugTarget for CoreTarget {
    fn busy(&self) -> bool {
        self.system.info().presentation_request.is_some()
    }
    fn tick(&mut self) {
        if let Some(token) = self.system.info().presentation_request {
            self.system.update(EventInput::PresentationReady(token));
        }
    }
    fn apply(&mut self, action: Action) -> Result<(), String> {
        let stamp = self.system.info().stamp;
        let input = match action {
            Action::Advance => EventInput::Advance { stamp },
            Action::Choose(choice) => EventInput::Choose { stamp, choice },
            Action::Complete => return Ok(()), // Core has no text playback.
            Action::Restart => {
                if !self.system.update(EventInput::Reset) {
                    return Err("reset rejected".into());
                }
                return self.start();
            }
        };
        if self.system.update(input) {
            Ok(())
        } else {
            Err("input rejected at current node".into())
        }
    }
    fn choices(&self) -> Vec<&str> {
        self.system
            .info()
            .choices
            .iter()
            .map(|c| c.id.as_str())
            .collect()
    }
    fn snapshot(&self) -> serde_json::Value {
        self.system.snapshot()
    }
    fn location(&self) -> String {
        let info = self.system.info();
        format!(
            "event={} flow={} node={}",
            info.event_id, info.flow_id, info.flow_node_id
        )
    }
}
fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut definition, mut event, mut commands) = (None, None, None);
    let mut plan = false;
    let mut limit = 100_000usize;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--definition" => definition = Some(args.next().ok_or("--definition requires a file")?),
            "--event" => event = Some(args.next().ok_or("--event requires an ID")?),
            "--commands" => commands = Some(args.next().ok_or("--commands requires a file")?),
            "--limit" => {
                limit = args
                    .next()
                    .ok_or("--limit requires a number")?
                    .parse()
                    .map_err(|_| "invalid limit")?
            }
            "--plan" => plan = true,
            "--help" => {
                println!("engine-debug-cli --definition <Core JSON> --event <ID> [--commands <file>] [--limit 100000]\nengine-debug-cli --plan --commands <file>\nCommands: advance, complete, choose, choose-id, show, check, until, restart, capture, quit");
                return Ok(());
            }
            _ => return Err(format!("unknown option: {arg}")),
        }
    }
    let reader: Box<dyn BufRead> = match commands {
        Some(path) => Box::new(BufReader::new(
            std::fs::File::open(path).map_err(|e| e.to_string())?,
        )),
        None => Box::new(BufReader::new(io::stdin())),
    };
    let mut input = CliInput::new(reader);
    if plan {
        let mut lines = Vec::new();
        while let Some(line) = input.next_command()? {
            let quit = matches!(line.command, Command::Quit);
            lines.push(line);
            if quit {
                break;
            }
        }
        println!(
            "{}",
            serde_json::to_string(&lines).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    let source = std::fs::read_to_string(definition.ok_or("--definition is required")?)
        .map_err(|e| e.to_string())?;
    let mut target = CoreTarget {
        system: JsonHolder::load(&source)?,
        entry: event.ok_or("--event is required")?,
    };
    target.start()?;
    let mut runner = Runner::<CoreTarget>::new(target, limit);
    runner.settle()?;
    println!("{}", runner.snapshot());
    while let Some(line) = input
        .next_command()
        .map_err(|e| format!("{}: {e}", runner.location()))?
    {
        match runner.execute(&line.command).map_err(|e| {
            format!(
                "line {} [{}] {}: {e}",
                line.line,
                line.source,
                runner.location()
            )
        })? {
            Outcome::Quit => break,
            Outcome::Checked => println!("OK line {} [{}]", line.line, line.source),
            Outcome::Capture(name) => println!("CAPTURE {name} (rendering adapter required)"),
            Outcome::Updated => println!("{}", runner.snapshot()),
        }
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
