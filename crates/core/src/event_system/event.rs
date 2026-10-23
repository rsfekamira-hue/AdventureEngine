//! Event definitions are immutable; progress belongs to the session.
use super::*;

#[derive(Clone)]
pub(super) struct Event {
    pub definition: std::sync::Arc<EventDefinition>,
    pub progress: EventProgress,
}

impl Event {
    pub fn new(definition: EventDefinition) -> Self {
        Self {
            definition: std::sync::Arc::new(definition),
            progress: EventProgress::default(),
        }
    }

    pub fn node(&self) -> &FlowNode {
        &self.definition.flow.as_ref().unwrap().flow_nodes[&self.progress.flow_node]
    }
}

#[derive(Clone, Default)]
pub(super) struct EventProgress {
    pub flow_node: String,
    pub selected: Option<FlowChoice>,
    pub added_choices: Vec<FlowChoice>,
    pub visits: i32,
    pub flow_generation: u64,
    pub choice_history: Vec<serde_json::Value>,
    pub finished: bool,
}

#[derive(Clone)]
pub struct EventStore {
    pub(super) entries: Vec<Event>,
    pub(super) ids: BTreeMap<String, EventId>,
}
