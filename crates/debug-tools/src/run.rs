//! Ordinary progression for a live debug host. The host owns frame updates.

pub enum RunMode<'a> {
    Settle,
    Choice,
    Target {
        event: &'a str,
        node: Option<&'a str>,
    },
}

pub struct RunState<'a> {
    pub event: &'a str,
    pub node: &'a str,
    pub busy: bool,
    pub ended: bool,
    pub has_choices: bool,
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStop {
    Settled,
    Target,
    Choice,
    End,
    Blocked,
    Limit,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunAction {
    Tick,
    Advance,
    Stop(RunStop),
}

impl RunMode<'_> {
    /// Choose an ordinary action or a stopping reason from the presented state.
    pub fn action(&self, state: RunState<'_>) -> RunAction {
        use RunAction::*;
        if state.busy {
            return Tick;
        }
        match self {
            Self::Settle => return Stop(RunStop::Settled),
            Self::Target { event, node }
                if *event == state.event && node.is_none_or(|node| node == state.node) =>
            {
                return Stop(RunStop::Target);
            }
            _ => {}
        }
        if state.ended {
            return Stop(RunStop::End);
        }
        if state.has_choices {
            return Stop(RunStop::Choice);
        }
        Advance
    }
}

/// Counts virtual steps. Waiting on the host's real frames does not spend steps.
#[derive(Default)]
pub struct RunSteps(u32);

impl RunSteps {
    pub fn count(&self) -> u32 {
        self.0
    }

    pub fn begin_step(&mut self, limit: u32) -> Result<(), RunStop> {
        if self.0 >= limit {
            return Err(RunStop::Limit);
        }
        self.0 += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progression_waits_for_presentation_and_stops_before_choices() {
        use RunAction::*;
        let state = || RunState {
            event: "event",
            node: "menu",
            busy: false,
            ended: false,
            has_choices: true,
        };
        let mode = RunMode::Target {
            event: "event",
            node: Some("menu"),
        };
        assert_eq!(
            mode.action(RunState {
                busy: true,
                ..state()
            }),
            Tick
        );
        assert_eq!(mode.action(state()), Stop(RunStop::Target));
        assert_eq!(RunMode::Choice.action(state()), Stop(RunStop::Choice));
        assert_eq!(RunMode::Settle.action(state()), Stop(RunStop::Settled));
        assert_eq!(
            RunMode::Choice.action(RunState {
                ended: true,
                ..state()
            }),
            Stop(RunStop::End)
        );
        assert_eq!(
            RunMode::Choice.action(RunState {
                has_choices: false,
                ..state()
            }),
            Advance
        );
        let mut steps = RunSteps::default();
        assert_eq!(steps.begin_step(1), Ok(()));
        assert_eq!(steps.begin_step(1), Err(RunStop::Limit));
        assert_eq!(steps.count(), 1);
    }
}
