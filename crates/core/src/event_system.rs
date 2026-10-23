//! Executes node transitions after parameter operations and effects have been resolved.
pub mod contracts;
mod debug;
mod definition;
mod event;
mod info;
mod interceptor;
#[cfg(feature = "debug-jump")]
pub mod jump;
mod loader;
pub mod migration;
mod parameters;
mod progress_gate;

pub use contracts::*;
pub use definition::*;
pub use event::EventStore;
pub use interceptor::{Interceptor, InterceptorStore};
pub use loader::JsonHolder;
pub use parameters::{ParameterSet, ParameterUpdate, ParameterValue};

use event::Event;
use progress_gate::ProgressGate;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub struct ParameterStore {
    initial: BTreeMap<String, ParameterSet>,
    sets: BTreeMap<String, ParameterSet>,
}

#[derive(Clone)]
struct ReturnPoint {
    event: EventId,
    destination: String,
    selected: Option<FlowChoice>,
    added_choices: Vec<FlowChoice>,
}

#[derive(Clone)]
pub struct EventSystem {
    events: EventStore,
    interceptors: InterceptorStore,
    parameters: ParameterStore,
    current_event: Option<EventId>,
    return_points: Vec<ReturnPoint>,
    condition_names: BTreeSet<String>,
    facts: ConditionFacts,
    info: EventSystemInfo,
    gate: ProgressGate,
    last_visible: Option<(EventId, String)>,
}

impl EventSystem {
    pub fn new(definition: EventSystemDefinition) -> Self {
        let ids = definition
            .events
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id.clone(), EventId::from_index(i)))
            .collect();
        Self {
            events: EventStore {
                entries: definition.events.into_iter().map(Event::new).collect(),
                ids,
            },
            interceptors: InterceptorStore {
                entries: std::sync::Arc::new(definition.interceptors),
            },
            parameters: ParameterStore {
                initial: definition.parameter_sets.clone(),
                sets: definition.parameter_sets,
            },
            current_event: None,
            return_points: Vec::new(),
            condition_names: definition.condition_names,
            facts: ConditionFacts::default(),
            info: EventSystemInfo::default(),
            gate: ProgressGate::default(),
            last_visible: None,
        }
    }

    pub fn info(&self) -> &EventSystemInfo {
        &self.info
    }

    pub fn required_conditions(&self) -> &BTreeSet<String> {
        &self.condition_names
    }

    pub fn event_id(&self, name: &str) -> Option<EventId> {
        self.events.ids.get(name).copied()
    }

    fn event(&self, id: EventId) -> &Event {
        &self.events.entries[id.index()]
    }

    fn event_mut(&mut self, id: EventId) -> &mut Event {
        &mut self.events.entries[id.index()]
    }

    fn node(&self, id: EventId) -> &FlowNode {
        self.event(id).node()
    }

    pub fn has_started(&self, id: EventId) -> bool {
        self.events
            .entries
            .get(id.index())
            .is_some_and(|e| e.progress.visits > 0)
    }

    pub fn parameter(&self, id: EventId, name: &str) -> Option<&ParameterValue> {
        self.parameter_set_value(
            &self
                .events
                .entries
                .get(id.index())?
                .definition
                .parameter_set,
            name,
        )
    }

    pub fn parameter_set_value(&self, set: &str, name: &str) -> Option<&ParameterValue> {
        self.parameters.sets.get(set)?.get(name)
    }

    fn conditions_ready(&self) -> bool {
        self.condition_names
            .iter()
            .all(|n| self.facts.get(n).is_some())
    }

    pub fn set_conditions(&mut self, facts: ConditionFacts) -> Result<(), String> {
        if self.condition_names.iter().any(|n| facts.get(n).is_none()) {
            return Err("条件結果がありません".into());
        }
        self.facts = facts;
        Ok(())
    }

    fn matches(&self, c: &Condition, choice: Option<&str>) -> bool {
        c.choice
            .as_deref()
            .is_none_or(|expected| Some(expected) == choice)
            && c.parameters.iter().all(|p| {
                self.parameters.sets[&p.parameter_set].matches(std::slice::from_ref(&p.comparison))
            })
            && c.external.iter().all(|c| c.matches(&self.facts))
    }

    fn choices(&self, id: EventId) -> Vec<FlowChoice> {
        if self.node(id).kind != FlowNodeKind::Choice {
            return Vec::new();
        }
        self.node(id)
            .choices
            .iter()
            .chain(&self.event(id).progress.added_choices)
            .cloned()
            .collect()
    }

    fn begin(&mut self, id: EventId, mode: EntryMode) {
        self.current_event = Some(id);
        self.info.status = SystemStatus::Active;
        if mode == EntryMode::Start {
            let start = self
                .event(id)
                .definition
                .flow
                .as_ref()
                .unwrap()
                .start_flow_node
                .clone();
            let p = &mut self.event_mut(id).progress;
            p.flow_node = start;
            p.selected = None;
            p.added_choices.clear();
            p.choice_history.clear();
            p.finished = false;
            p.visits += 1;
            p.flow_generation += 1;
        }
    }

    fn next(&self, id: EventId) -> Option<String> {
        match &self.node(id).next {
            Some(NextNode::Fixed { node }) => Some(node.clone()),
            Some(NextNode::SelectedDestination) => self
                .event(id)
                .progress
                .selected
                .as_ref()
                .map(|c| c.destination.clone()),
            None => None,
        }
    }

    /// Evaluate operation conditions against the same pre-operation state, then
    /// evaluate all effects against the resulting state. Declaration order is
    /// used only for applying operations, not for selecting a transition.
    fn transition(&mut self, id: EventId, to: String) {
        let from = self.event(id).progress.flow_node.clone();
        let event_name = self.event(id).definition.id.clone();
        let choice = if self.node(id).kind == FlowNodeKind::Choice {
            self.event(id)
                .progress
                .selected
                .as_ref()
                .map(|c| c.id.clone())
        } else {
            None
        };
        let interventions: Vec<_> = self
            .event(id)
            .definition
            .interventions
            .iter()
            .chain(
                self.interceptors
                    .entries
                    .iter()
                    .flat_map(|i| &i.interventions),
            )
            .filter(|i| i.target.event == event_name && i.target.from == from && i.target.to == to)
            .cloned()
            .collect();
        let operations: Vec<_> = interventions
            .iter()
            .filter_map(|i| i.parameter_operation.as_ref())
            .filter(|p| self.matches(&p.condition, choice.as_deref()))
            .map(|p| p.operation.clone())
            .collect();
        for op in operations {
            self.parameters
                .sets
                .get_mut(&op.parameter_set)
                .unwrap()
                .apply(&[op.update])
                .expect("validated parameter operation");
        }
        let effects: Vec<_> = interventions
            .iter()
            .filter_map(|i| i.effect.as_ref())
            .filter(|e| self.matches(&e.condition, choice.as_deref()))
            .map(|e| e.effect.clone())
            .collect();
        let mut requested = Vec::new();
        let mut additions = Vec::new();
        for effect in effects {
            match effect {
                Effect::AddChoice { choice } => additions.push(choice),
                Effect::CallEvent {
                    event,
                    priority,
                    entry_mode,
                } => requested.push((priority, event, entry_mode, true)),
                Effect::SwitchEvent {
                    event,
                    priority,
                    entry_mode,
                } => requested.push((priority, event, entry_mode, false)),
            }
        }
        requested.retain(|(_, name, mode, _)| {
            let e = self.event(self.event_id(name).unwrap());
            *mode == EntryMode::Start || (e.progress.visits > 0 && !e.progress.finished)
        });
        // Preserve legacy priority and deterministic target-name tie breaking.
        requested.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        if self.event(id).definition.flow.as_ref().unwrap().flow_nodes[&to].kind
            == FlowNodeKind::Choice
        {
            self.event_mut(id).progress.added_choices = additions;
        }
        if let Some((_, name, mode, call)) = requested.into_iter().next() {
            let target = self.event_id(&name).unwrap();
            if call && target != id {
                self.return_points.push(ReturnPoint {
                    event: id,
                    destination: to,
                    selected: self.event(id).progress.selected.clone(),
                    added_choices: self.event(id).progress.added_choices.clone(),
                });
            } else {
                self.event_mut(id).progress.finished = true;
            }
            self.begin(target, mode);
        } else {
            self.event_mut(id).progress.flow_node = to;
        }
    }

    /// Finish one event without consuming the resumed node's automatic edges.
    fn finish_event(&mut self, id: EventId) -> bool {
        self.event_mut(id).progress.finished = true;
        if self.event(id).definition.end_behavior == EventEndBehavior::Return {
            if let Some(point) = self.return_points.pop() {
                let p = &mut self.event_mut(point.event).progress;
                p.flow_node = point.destination;
                p.selected = point.selected;
                p.added_choices = point.added_choices;
                self.begin(point.event, EntryMode::Resume);
                return true;
            }
        }
        self.return_points.clear();
        self.info.status = SystemStatus::Completed;
        false
    }

    fn settle(&mut self) {
        let mut visited = BTreeSet::new();
        while let Some(id) = self.current_event {
            if !visited.insert((id, self.event(id).progress.flow_node.clone())) {
                self.info.status = SystemStatus::Completed;
                self.return_points.clear();
                break;
            }
            match self.node(id).kind {
                FlowNodeKind::Auto => {
                    let Some(next) = self.next(id) else {
                        self.info.status = SystemStatus::Completed;
                        break;
                    };
                    self.transition(id, next);
                }
                FlowNodeKind::End => {
                    if self.finish_event(id) {
                        continue;
                    }
                    break;
                }
                _ => {
                    self.last_visible = Some((id, self.event(id).progress.flow_node.clone()));
                    break;
                }
            }
        }
    }

    pub fn update(&mut self, input: EventInput) -> bool {
        if matches!(input, EventInput::Reset) {
            self.parameters.sets = self.parameters.initial.clone();
            for e in &mut self.events.entries {
                e.progress = Default::default();
            }
            self.current_event = None;
            self.return_points.clear();
            self.last_visible = None;
            self.gate = ProgressGate::Open;
            self.info = EventSystemInfo {
                stamp: FlowStamp {
                    generation: self.info.stamp.generation + 1,
                    revision: self.info.stamp.revision + 1,
                },
                ..Default::default()
            };
            return true;
        }
        if let EventInput::Start { event, mode } = input {
            if self
                .events
                .entries
                .get(event.index())
                .is_none_or(|e| e.definition.flow.is_none())
                || !self.conditions_ready()
                || (mode == EntryMode::Resume
                    && (!self.has_started(event) || self.event(event).progress.finished))
            {
                return false;
            }
            self.return_points.clear();
            self.begin(event, mode);
            self.settle();
            self.refresh();
            return true;
        }
        if let EventInput::PresentationReady(request) = input {
            if !self.gate.finish(request) {
                return false;
            }
            self.info.presentation_request = self.gate.request();
            if let Some(id) = self.current_event {
                if self.info.status == SystemStatus::Active
                    && self.event(id).definition.end_behavior == EventEndBehavior::End
                    && self.next(id).as_deref() == Some(POST_EVENT)
                    && self.node(id).kind == FlowNodeKind::Advance
                {
                    self.transition(id, POST_EVENT.into());
                    self.settle();
                    self.refresh();
                }
            }
            return true;
        }
        let Some(id) = self.current_event else {
            return false;
        };
        if self.info.status != SystemStatus::Active
            || !self.conditions_ready()
            || self.gate.request().is_some()
        {
            return false;
        }
        match input {
            EventInput::Choose { stamp, choice }
                if stamp == self.info.stamp && self.node(id).kind == FlowNodeKind::Choice =>
            {
                let Some(selected) = self.choices(id).into_iter().find(|c| c.id == choice) else {
                    return false;
                };
                let from = self.event(id).progress.flow_node.clone();
                let p = &mut self.event_mut(id).progress;
                p.choice_history
                    .push(serde_json::json!({"flowNode":from,"choice":choice}));
                p.selected = Some(selected);
            }
            EventInput::Advance { stamp }
                if stamp == self.info.stamp && self.node(id).kind == FlowNodeKind::Advance => {}
            _ => return false,
        }
        let Some(next) = self.next(id) else {
            return false;
        };
        self.transition(id, next);
        self.settle();
        self.refresh();
        true
    }
}
