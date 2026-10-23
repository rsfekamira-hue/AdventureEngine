//! An interceptor references events; EventStore owns their instances.
use super::Intervention;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Interceptor {
    pub id: String,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub interventions: Vec<Intervention>,
}

#[derive(Clone)]
pub struct InterceptorStore {
    pub(super) entries: std::sync::Arc<Vec<Interceptor>>,
}
