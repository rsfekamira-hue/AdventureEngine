use engine_debug_tools::{
    input::{CliInput, Command},
    replay::{Action, DebugTarget, Outcome, Runner},
};
use serde_json::json;
use std::io::Cursor;

// An application with no engine/game dependency proves the adapter boundary.
#[derive(Default)]
struct Counter {
    count: u32,
    busy: bool,
}
impl DebugTarget for Counter {
    fn busy(&self) -> bool {
        self.busy
    }
    fn tick(&mut self) {
        self.busy = false;
    }
    fn apply(&mut self, action: Action) -> Result<(), String> {
        match action {
            Action::Advance if self.count < 2 => self.count += 1,
            Action::Choose(id) if id == "next" => self.count += 1,
            Action::Restart => self.count = 0,
            Action::Complete => {}
            _ => return Err("cannot advance".into()),
        }
        self.busy = true;
        Ok(())
    }
    fn choices(&self) -> Vec<&str> {
        vec!["next"]
    }
    fn snapshot(&self) -> serde_json::Value {
        json!({"count": self.count, "nullable": null})
    }
    fn location(&self) -> String {
        format!("count={}", self.count)
    }
}
#[test]
fn shared_commands_work_with_an_unrelated_application() {
    let mut runner = Runner::<Counter>::new(Counter::default(), 50);
    let mut input = CliInput::new(Cursor::new("\u{feff}# comment\n\nchoose 1\ncheck /count 1\nuntil /count 2\ncapture done\nrestart\ncheck /count 0\nquit\n"));
    let mut outcomes = Vec::new();
    while let Some(line) = input.next_command().unwrap() {
        if outcomes.is_empty() {
            assert_eq!(line.line, 3);
        }
        outcomes.push(runner.execute(&line.command).unwrap());
    }
    assert!(outcomes.contains(&Outcome::Capture("done".into())));
    assert_eq!(outcomes.last(), Some(&Outcome::Quit));
    assert_eq!(runner.snapshot()["count"], 0);
}
#[test]
fn missing_values_rejected_choices_and_unreachable_targets_fail() {
    let mut runner = Runner::<Counter>::new(Counter::default(), 50);
    assert!(runner
        .execute(&Command::Check {
            pointer: "/absent".into(),
            expected: json!(null)
        })
        .unwrap_err()
        .contains("missing"));
    runner
        .execute(&Command::Check {
            pointer: "/nullable".into(),
            expected: json!(null),
        })
        .unwrap();
    for value in ["0", "2", "unknown"] {
        assert!(runner
            .execute(&Command::Choose {
                value: value.into(),
                numbered: true
            })
            .is_err());
    }
    assert_eq!(runner.snapshot()["count"], 0);
    assert!(runner
        .execute(&Command::Until {
            pointer: "/count".into(),
            expected: json!(99)
        })
        .unwrap_err()
        .contains("cannot reach"));
}
#[test]
fn command_action_and_virtual_frames_share_the_budget() {
    let mut runner = Runner::<Counter>::new(Counter::default(), 2);
    // The command and action use two steps; its pending frame must not run.
    assert!(runner
        .execute(&Command::Advance { count: 1 })
        .unwrap_err()
        .contains("limit"));
    assert!(runner.target().busy());
}
#[test]
fn invalid_commands_fail_during_parsing() {
    for source in [
        "advance 0",
        "advance -1",
        "choose",
        "check /x bad",
        "check x 1",
        "capture ../x",
        "quit extra",
    ] {
        assert!(CliInput::<Cursor<&str>>::parse(source).is_err(), "{source}");
    }
}

struct Host {
    captures: Vec<u32>,
    fail: bool,
}
impl engine_debug_tools::cli::CliHost<Counter> for Host {
    fn display(&mut self, target: &Counter, out: &mut dyn std::io::Write) -> Result<(), String> {
        writeln!(out, "display {}", target.count).map_err(|e| e.to_string())
    }
    fn capture(&mut self, target: &Counter, name: &str) -> Result<String, String> {
        assert!(!target.busy);
        if self.fail {
            return Err("capture failed".into());
        }
        self.captures.push(target.count);
        Ok(format!("{name}.png"))
    }
}
#[test]
fn cli_quiet_preserves_show_checks_and_settled_capture() {
    let mut runner = Runner::<Counter>::new(Counter::default(), 50);
    let mut host = Host {
        captures: vec![],
        fail: false,
    };
    let mut output = Vec::new();
    let mut input = CliInput::new(Cursor::new(
        "advance\nshow\ncheck /count 1\ncapture scene\nquit\nadvance\n",
    ));
    engine_debug_tools::cli::run(&mut runner, &mut input, &mut host, &mut output, true).unwrap();
    let text = String::from_utf8(output).unwrap();
    assert!(!text.contains("display"));
    assert!(text.contains("\"count\":1"));
    assert!(text.contains("OK line 3"));
    assert!(text.contains("CAPTURE scene.png"));
    assert_eq!(host.captures, [1]);
    assert_eq!(runner.snapshot()["count"], 1);
}
#[test]
fn cli_displays_initial_state_and_contextualizes_capture_errors() {
    let mut runner = Runner::<Counter>::new(Counter::default(), 50);
    let mut host = Host {
        captures: vec![],
        fail: true,
    };
    let mut output = Vec::new();
    let mut input = CliInput::new(Cursor::new("advance\ncapture broken\n"));
    let error =
        engine_debug_tools::cli::run(&mut runner, &mut input, &mut host, &mut output, false)
            .unwrap_err();
    assert_eq!(String::from_utf8(output).unwrap(), "display 0\ndisplay 1\n");
    for part in ["line 2", "capture broken", "count=1", "capture failed"] {
        assert!(error.contains(part), "{error}");
    }
}
