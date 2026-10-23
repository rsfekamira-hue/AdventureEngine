//! 進行の入力と情報。画面・人物・作品の役割を含めない。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowStamp {
    pub generation: u64,
    pub revision: u64,
}
/// 配列の添字をイベントの識別子として型で区別する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventId(usize);
impl EventId {
    pub(super) fn from_index(index: usize) -> Self {
        Self(index)
    }
    pub(super) fn index(self) -> usize {
        self.0
    }
}
#[derive(Clone, Copy, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryMode {
    #[default]
    Start,
    Resume,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemStatus {
    #[default]
    Idle,
    Active,
    Completed,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct EventChoice {
    pub id: String,
}
#[derive(Clone, Debug, Default)]
pub struct EventSystemInfo {
    pub stamp: FlowStamp,
    pub status: SystemStatus,
    pub event_id: String,
    pub flow_id: String,
    pub flow_node_id: String,
    pub flow_generation: u64,
    pub choices: Vec<EventChoice>,
    pub presentation_request: Option<u64>,
}
#[derive(Clone, Debug)]
pub enum EventInput {
    Start { event: EventId, mode: EntryMode },
    Advance { stamp: FlowStamp },
    Choose { stamp: FlowStamp, choice: String },
    PresentationReady(u64),
    Reset,
}
/// 名前はデータとの接続口。人物属性そのものを辞書へ格納しない。
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct ConditionFacts {
    values: BTreeMap<String, bool>,
}
impl ConditionFacts {
    pub fn set(&mut self, name: impl Into<String>, value: bool) {
        self.values.insert(name.into(), value);
    }
    pub fn get(&self, name: &str) -> Option<bool> {
        self.values.get(name).copied()
    }
}
fn expected_true() -> bool {
    true
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalCondition {
    pub name: String,
    #[serde(default = "expected_true")]
    pub equals: bool,
}
impl ExternalCondition {
    pub(crate) fn matches(&self, facts: &ConditionFacts) -> bool {
        facts.get(&self.name) == Some(self.equals)
    }
}
