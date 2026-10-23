//! Definitions shared by the loader and the transition runtime.
use super::parameters::{Condition as ParameterCondition, ParameterSet, ParameterUpdate};
use super::{EntryMode, ExternalCondition};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PRE_EVENT: &str = "@event:start";
pub const POST_EVENT: &str = "@event:end";
pub const FINISH: &str = "@event:finish";

pub fn pre_choice(node: &str) -> String {
    format!("@choice:pre:{node}")
}

pub fn post_choice(node: &str) -> String {
    format!("@choice:post:{node}")
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub id: String,
    pub start_flow_node: String,
    pub flow_nodes: BTreeMap<String, FlowNode>,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlowNodeKind {
    Auto,
    Advance,
    Choice,
    End,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NextNode {
    Fixed { node: String },
    SelectedDestination,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlowNode {
    pub kind: FlowNodeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<NextNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<FlowChoice>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub generated: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlowChoice {
    pub id: String,
    pub destination: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WatchTarget {
    pub event: String,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Condition {
    pub parameters: Vec<SharedCondition>,
    pub external: Vec<ExternalCondition>,
    pub choice: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SharedCondition {
    pub parameter_set: String,
    pub comparison: ParameterCondition,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub parameter_set: String,
    pub update: ParameterUpdate,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalParameterOperation {
    pub condition: Condition,
    pub operation: Operation,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalEffect {
    pub condition: Condition,
    pub effect: Effect,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    CallEvent {
        event: String,
        #[serde(default)]
        priority: i32,
        #[serde(default)]
        entry_mode: EntryMode,
    },
    SwitchEvent {
        event: String,
        #[serde(default)]
        priority: i32,
        #[serde(default)]
        entry_mode: EntryMode,
    },
    AddChoice {
        choice: FlowChoice,
    },
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Intervention {
    pub target: WatchTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_operation: Option<ConditionalParameterOperation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<ConditionalEffect>,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventEndBehavior {
    #[default]
    Return,
    End,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventDefinitionJson {
    pub id: String,
    pub flow: Option<String>,
    pub parameter_set: String,
    #[serde(default)]
    pub interventions: Vec<Intervention>,
    #[serde(default)]
    pub end_behavior: EventEndBehavior,
}

#[derive(Clone)]
pub struct EventDefinition {
    pub(super) id: String,
    pub(super) flow: Option<Flow>,
    pub(super) parameter_set: String,
    pub(super) interventions: Vec<Intervention>,
    pub(super) end_behavior: EventEndBehavior,
}

impl EventDefinition {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn flow(&self) -> Option<&Flow> {
        self.flow.as_ref()
    }

    pub fn parameter_set_id(&self) -> &str {
        &self.parameter_set
    }
}

pub struct EventSystemDefinition {
    pub(super) events: Vec<EventDefinition>,
    pub(super) interceptors: Vec<super::interceptor::Interceptor>,
    pub(super) parameter_sets: BTreeMap<String, ParameterSet>,
    pub(super) condition_names: BTreeSet<String>,
}
