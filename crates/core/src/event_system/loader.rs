//! Validates references before constructing an executable session.
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonHolder {
    schema_version: u32,
    flows: Vec<Flow>,
    events: Vec<definition::EventDefinitionJson>,
    #[serde(default)]
    interceptors: Vec<interceptor::Interceptor>,
    parameter_sets: BTreeMap<String, ParameterSet>,
    #[serde(default)]
    external_conditions: Vec<String>,
}

impl JsonHolder {
    pub fn load(input: &str) -> Result<EventSystem, String> {
        let value = migration::migrate_definition(
            serde_json::from_str(input).map_err(|e| format!("{e}"))?,
        )?;
        let holder: Self = serde_json::from_value(value).map_err(|e| e.to_string())?;
        if holder.schema_version != 3 {
            return Err("Coreイベント形式はschema_version: 3です".into());
        }
        let names: BTreeSet<_> = holder.external_conditions.iter().cloned().collect();
        if names.len() != holder.external_conditions.len() || names.iter().any(String::is_empty) {
            return Err("外部条件名が空または重複しています".into());
        }
        for (id, p) in &holder.parameter_sets {
            if id.is_empty() {
                return Err("ParameterSet IDが空です".into());
            }
            p.validate_names()?;
        }
        let mut flows = BTreeMap::new();
        for flow in holder.flows {
            if flow.id.is_empty() || !flow.flow_nodes.contains_key(&flow.start_flow_node) {
                return Err("Flowの開始ノードがありません".into());
            }
            for (id, n) in &flow.flow_nodes {
                if id.is_empty() {
                    return Err("FlowNode IDが空です".into());
                }
                if (n.kind == FlowNodeKind::End) != n.next.is_none() {
                    return Err("FlowNodeの続きが不正です".into());
                }
                if let Some(NextNode::Fixed { node }) = &n.next {
                    if !flow.flow_nodes.contains_key(node) {
                        return Err(format!(
                            "続きのFlowNodeがありません: {}/{id} → {node}",
                            flow.id
                        ));
                    }
                }
                if !n.choices.is_empty() && n.kind != FlowNodeKind::Choice {
                    return Err("選択肢はChoice専用です".into());
                }
                let mut ids = BTreeSet::new();
                for c in &n.choices {
                    if c.id.is_empty()
                        || !ids.insert(&c.id)
                        || !flow.flow_nodes.contains_key(&c.destination)
                    {
                        return Err("選択肢のID・続きが不正です".into());
                    }
                }
                if matches!(n.next, Some(NextNode::SelectedDestination))
                    && !flow.flow_nodes.values().any(|c| {
                        c.kind == FlowNodeKind::Choice
                            && matches!(&c.next,Some(NextNode::Fixed{node}) if node==id)
                    })
                {
                    return Err("選択結果のないPostChoiceです".into());
                }
            }
            if flows.insert(flow.id.clone(), flow).is_some() {
                return Err("Flow IDが重複しています".into());
            }
        }
        let mut source = holder.events;
        source.sort_by(|a, b| a.id.cmp(&b.id));
        let mut events = Vec::new();
        let mut ids = BTreeSet::new();
        for e in source {
            if e.id.is_empty() || !ids.insert(e.id.clone()) {
                return Err("Event IDが空または重複しています".into());
            }
            if !holder.parameter_sets.contains_key(&e.parameter_set) {
                return Err("ParameterSetがありません".into());
            }
            let flow = e
                .flow
                .map(|f| flows.remove(&f).ok_or("EventとFlowは一対一です"))
                .transpose()?;
            events.push(EventDefinition {
                id: e.id,
                flow,
                parameter_set: e.parameter_set,
                interventions: e.interventions,
                end_behavior: e.end_behavior,
            });
        }
        if !flows.is_empty() {
            return Err("Eventに所属しないFlowがあります".into());
        }
        let by_id: BTreeMap<_, _> = events.iter().map(|e| (e.id.as_str(), e)).collect();
        let mut choices: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
        for e in &events {
            if e.interventions.iter().any(|i| i.target.event != e.id) {
                return Err(format!(
                    "Event自身のInterventionは自身のFlowを監視します: {}",
                    e.id
                ));
            }
            if let Some(f) = &e.flow {
                for (n, v) in &f.flow_nodes {
                    choices.insert(
                        (e.id.clone(), n.clone()),
                        v.choices.iter().map(|c| c.id.clone()).collect(),
                    );
                }
            }
        }
        let mut interceptors = holder.interceptors;
        interceptors.sort_by(|a, b| a.id.cmp(&b.id));
        let mut interceptor_ids = BTreeSet::new();
        for i in &interceptors {
            if i.id.is_empty()
                || !interceptor_ids.insert(&i.id)
                || i.event
                    .as_ref()
                    .is_some_and(|e| !by_id.contains_key(e.as_str()))
            {
                return Err("Interceptor ID・関連Eventが不正です".into());
            }
        }
        let all: Vec<_> = events
            .iter()
            .flat_map(|e| &e.interventions)
            .chain(interceptors.iter().flat_map(|i| &i.interventions))
            .collect();
        for i in &all {
            let flow = by_id
                .get(i.target.event.as_str())
                .and_then(|e| e.flow.as_ref())
                .ok_or("監視対象Event・Flowがありません")?;
            let from = flow
                .flow_nodes
                .get(&i.target.from)
                .ok_or("移動元がありません")?;
            let to = flow
                .flow_nodes
                .get(&i.target.to)
                .ok_or("移動先がありません")?;
            let connected = match &from.next {
                Some(NextNode::Fixed { node }) => node == &i.target.to,
                Some(NextNode::SelectedDestination) => true,
                None => false,
            };
            if !connected {
                return Err("WatchTargetの接続がありません".into());
            }
            if let Some(p) = &i.parameter_operation {
                validate_condition(&p.condition, &holder.parameter_sets, &names)?;
                holder
                    .parameter_sets
                    .get(&p.operation.parameter_set)
                    .ok_or("操作のParameterSetがありません")?
                    .validate_update(&p.operation.update)?;
            }
            if let Some(e) = &i.effect {
                validate_condition(&e.condition, &holder.parameter_sets, &names)?;
                match &e.effect {
                    Effect::CallEvent { event, .. } | Effect::SwitchEvent { event, .. } => {
                        if by_id
                            .get(event.as_str())
                            .and_then(|e| e.flow.as_ref())
                            .is_none()
                        {
                            return Err("遷移先EventにFlowがありません".into());
                        }
                    }
                    Effect::AddChoice { choice } => {
                        if to.kind != FlowNodeKind::Choice
                            || from.kind != FlowNodeKind::Auto
                            || choice.id.is_empty()
                            || !flow.flow_nodes.contains_key(&choice.destination)
                            || !choices
                                .get_mut(&(i.target.event.clone(), i.target.to.clone()))
                                .unwrap()
                                .insert(choice.id.clone())
                        {
                            return Err("追加選択肢の対象・ID・続きが不正です".into());
                        }
                    }
                }
            }
        }
        for i in &all {
            for c in i
                .parameter_operation
                .iter()
                .map(|p| &p.condition)
                .chain(i.effect.iter().map(|e| &e.condition))
            {
                if let Some(choice) = &c.choice {
                    if !choices[&(i.target.event.clone(), i.target.from.clone())].contains(choice) {
                        return Err("条件の選択肢がありません".into());
                    }
                }
            }
        }
        Ok(EventSystem::new(EventSystemDefinition {
            events,
            interceptors,
            parameter_sets: holder.parameter_sets,
            condition_names: names,
        }))
    }
}

fn validate_condition(
    c: &Condition,
    sets: &BTreeMap<String, ParameterSet>,
    names: &BTreeSet<String>,
) -> Result<(), String> {
    for p in &c.parameters {
        sets.get(&p.parameter_set)
            .ok_or("条件のParameterSetがありません")?
            .validate_condition(&p.comparison)?;
    }
    if c.external.iter().any(|e| !names.contains(&e.name)) {
        return Err("未登録の外部条件です".into());
    }
    Ok(())
}
