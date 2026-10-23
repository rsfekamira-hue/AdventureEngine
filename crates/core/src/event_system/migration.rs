//! One-way conversion of schema 2 definitions. Runtime execution uses only schema 3.
use super::*;
use serde_json::{json, Value};

fn list(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn string(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn condition(local: &Value, external: &Value, set: &str, choice: Option<&str>) -> Value {
    let parameters: Vec<_> = list(local)
        .into_iter()
        .map(|mut c| {
            if c.get("parameter_set").is_none() {
                c["parameter_set"] = json!(set);
            }
            let set = c.as_object_mut().unwrap().remove("parameter_set").unwrap();
            json!({"parameter_set":set,"comparison":c})
        })
        .collect();
    json!({"parameters": parameters, "external": list(external), "choice": choice})
}

fn watch(event: &str, from: &str, to: &str) -> Value {
    json!({"event":event,"from":from,"to":to})
}

fn operations(out: &mut Vec<Value>, target: &Value, c: &Value, updates: Vec<Value>, set: &str) {
    for mut update in updates {
        update.as_object_mut().unwrap().remove("type");
        if update.get("parameter_set").is_none() {
            update["parameter_set"] = json!(set);
        }
        let set = update
            .as_object_mut()
            .unwrap()
            .remove("parameter_set")
            .unwrap();
        out.push(json!({
            "target": target,
            "parameter_operation": {
                "condition": c,
                "operation": { "parameter_set": set, "update": update },
            },
        }));
    }
}

fn actions(event: &Value, names: &Value) -> Result<Vec<Value>, String> {
    let mut result = Vec::new();
    for name in list(names) {
        let action = event["event_actions"]
            .get(string(&name))
            .ok_or("EventActionがありません")?;
        result.extend(list(&action["parameter_updates"]));
    }
    Ok(result)
}

/// Used by both the migration CLI and the legacy fixture import boundary.
pub fn migrate_definition(mut root: Value) -> Result<Value, String> {
    if root["schema_version"] == 3 {
        return Ok(root);
    }
    if root["schema_version"] != 2 {
        return Err("Coreイベント形式はschema_version: 3です".into());
    }
    let events = list(&root["events"]);
    let mut old_flows = BTreeMap::new();
    let mut flows = BTreeMap::new();
    for value in list(&root["flows"]) {
        let id = string(&value["id"]);
        if old_flows.insert(id.clone(), value.clone()).is_some() {
            return Err("Flow IDが重複しています".into());
        }
        let source = value["flow_nodes"]
            .as_object()
            .ok_or("FlowNode一覧がありません")?;
        if source.values().any(|n| n.get("kind").is_some()) {
            flows.insert(id, value.clone());
            continue;
        }
        if source.keys().any(|n| n.starts_with('@')) {
            return Err("生成ノードの予約IDです".into());
        }
        let entry = |name: &str| {
            if source
                .get(name)
                .is_some_and(|n| !list(&n["choices"]).is_empty())
            {
                pre_choice(name)
            } else {
                name.to_owned()
            }
        };
        let mut nodes = serde_json::Map::new();
        for (name, node) in source {
            if !node["external_system_call"].is_null() {
                return Err("外部システム連携は廃止されました".into());
            }
            let choices = list(&node["choices"]);
            if (!choices.is_empty() && node["next_flow_node"].is_string())
                || (node["finish_on_enter"] == true
                    && (!choices.is_empty() || node["next_flow_node"].is_string()))
            {
                return Err("FlowNodeの選択・続き・終了接続点は併用できません".into());
            }
            let kind = if node["finish_on_enter"] == true {
                "auto"
            } else if choices.is_empty() {
                "advance"
            } else {
                "choice"
            };
            let to = if choices.is_empty() {
                node["next_flow_node"]
                    .as_str()
                    .map(&entry)
                    .unwrap_or(POST_EVENT.into())
            } else {
                post_choice(name)
            };
            let choices: Vec<_> = choices
                .iter()
                .map(|c| json!({"id":c["id"],"destination":entry(&string(&c["next_flow_node"]))}))
                .collect();
            nodes.insert(
                name.clone(),
                json!({"kind":kind,"next":{"type":"fixed","node":to},"choices":choices}),
            );
            if kind == "choice" {
                nodes.insert(
                    pre_choice(name),
                    json!({"kind":"auto","generated":true,"next":{"type":"fixed","node":name}}),
                );
                nodes.insert(
                    post_choice(name),
                    json!({"kind":"auto","generated":true,"next":{"type":"selected_destination"}}),
                );
            }
        }
        nodes.insert(
            PRE_EVENT.into(),
            json!({
                "kind": "auto",
                "generated": true,
                "next": {
                    "type": "fixed",
                    "node": entry(&string(&value["start_flow_node"])),
                },
            }),
        );
        nodes.insert(
            POST_EVENT.into(),
            json!({"kind":"auto","generated":true,"next":{"type":"fixed","node":FINISH}}),
        );
        nodes.insert(FINISH.into(), json!({"kind":"end","generated":true}));
        flows.insert(
            id.clone(),
            json!({"id":id,"start_flow_node":PRE_EVENT,"flow_nodes":nodes}),
        );
    }
    let owned: BTreeMap<_, _> = events
        .iter()
        .map(|e| (string(&e["id"]), string(&e["flow"])))
        .collect();
    let mut converted_events = Vec::new();
    let mut converted_interceptors = Vec::new();
    for event in &events {
        let id = string(&event["id"]);
        let set = string(&event["parameter_set"]);
        let mut own = list(&event["interventions"]);
        let mut external = Vec::new();
        let empty = condition(&Value::Null, &Value::Null, &set, None);
        if let Some(flow) = flows.get(&string(&event["flow"])) {
            let nodes = flow["flow_nodes"].as_object().unwrap();
            let start = string(&nodes[PRE_EVENT]["next"]["node"]);
            operations(
                &mut own,
                &watch(&id, PRE_EVENT, &start),
                &empty,
                actions(event, &event["on_enter_actions"])?,
                &set,
            );
            operations(
                &mut own,
                &watch(&id, POST_EVENT, FINISH),
                &empty,
                actions(event, &event["on_end_actions"])?,
                &set,
            );
            let old = &old_flows[&string(&event["flow"])];
            for (name, node) in old["flow_nodes"].as_object().unwrap() {
                for (from, to) in connections(flow) {
                    let arrival = if !list(&node["choices"]).is_empty() {
                        pre_choice(name)
                    } else {
                        name.clone()
                    };
                    if to == arrival {
                        operations(
                            &mut own,
                            &watch(&id, &from, &to),
                            &empty,
                            actions(event, &node["on_enter_actions"])?,
                            &set,
                        );
                    }
                }
                for c in list(&node["choices"]) {
                    let cond = condition(&Value::Null, &Value::Null, &set, c["id"].as_str());
                    operations(
                        &mut own,
                        &watch(&id, name, &post_choice(name)),
                        &cond,
                        actions(event, &c["event_actions"])?,
                        &set,
                    );
                }
            }
        }
        for r in list(&event["event_transition_rules"]) {
            let from_event = string(&r["from_event"]);
            let flow = flows
                .get(owned.get(&from_event).ok_or("遷移元Eventがありません")?)
                .ok_or("遷移元Flowがありません")?;
            let c = condition(&r["conditions"], &r["external_conditions"], &set, None);
            let timing = string(&r["check"]);
            let old_node = string(&r["flow_node"]);
            for (from, to) in connections(flow) {
                let matches = match timing.as_str() {
                    "event_start" => from == PRE_EVENT,
                    "event_end" => from == POST_EVENT,
                    "flow_node" => {
                        let incoming = if flow["flow_nodes"].get(pre_choice(&old_node)).is_some() {
                            pre_choice(&old_node)
                        } else {
                            old_node.clone()
                        };
                        from == old_node || (incoming == old_node && to == incoming)
                    }
                    _ => return Err("不明な遷移判定です".into()),
                };
                if matches {
                    let kind = if timing == "flow_node" && from_event != id {
                        "call_event"
                    } else {
                        "switch_event"
                    };
                    let effect = json!({
                        "type": kind,
                        "event": id,
                        "priority": r["priority"].as_i64().unwrap_or(0),
                        "entry_mode": r["entry_mode"].as_str().unwrap_or("start"),
                    });
                    let i = json!({"target":watch(&from_event,&from,&to),"effect":{"condition":c,"effect":effect}});
                    if from_event == id {
                        own.push(i);
                    } else {
                        external.push(i);
                    }
                }
            }
        }
        // Old event-owned observers are converted at the boundary as well.
        for w in list(&event["watcher_event_actions"]) {
            if w["observation"]["type"] != "choice_selected" {
                return Err("外部システム監視は廃止されました".into());
            }
            let node = string(&w["flow_node"]);
            let c = condition(
                &Value::Null,
                &Value::Null,
                &set,
                w["observation"]["choice"].as_str(),
            );
            operations(
                &mut external,
                &watch(&string(&w["watched_event"]), &node, &post_choice(&node)),
                &c,
                actions(event, &w["event_actions"])?,
                &set,
            );
        }
        for w in list(&event["watcher_choices"]) {
            let node = string(&w["flow_node"]);
            let target_event = string(&w["watched_event"]);
            let flow = flows
                .get(&owned[&target_event])
                .ok_or("監視先Flowがありません")?;
            let dest = entry(flow, &string(&w["choice"]["next_flow_node"]));
            let choice = json!({"id":w["choice"]["id"],"destination":dest});
            external.push(json!({
                "target": watch(&target_event, &pre_choice(&node), &node),
                "effect": {
                    "condition": condition(&w["conditions"], &w["external_conditions"], &set, None),
                    "effect": { "type": "add_choice", "choice": choice },
                },
            }));
            operations(
                &mut external,
                &watch(&target_event, &node, &post_choice(&node)),
                &condition(&Value::Null, &Value::Null, &set, w["choice"]["id"].as_str()),
                actions(event, &w["choice"]["event_actions"])?,
                &set,
            );
        }
        if !external.is_empty() {
            converted_interceptors
                .push(json!({"id":format!("transition:{id}"),"event":id,"interventions":external}));
        }
        converted_events.push(json!({
            "id": id,
            "flow": event["flow"],
            "parameter_set": set,
            "interventions": own,
            "end_behavior": event["end_behavior"].as_str().unwrap_or("return"),
        }));
    }
    for interceptor in list(&root["interceptors"]) {
        let mut interventions = Vec::new();
        if interceptor.get("interventions").is_some() {
            converted_interceptors.push(interceptor);
            continue;
        }
        let mut rule_ids = BTreeSet::new();
        for r in list(&interceptor["rules"]) {
            if string(&r["id"]).is_empty() || !rule_ids.insert(string(&r["id"])) {
                return Err("介入ルールIDが空または重複しています".into());
            }
            let t = &r["trigger"];
            let event = string(&t["event"]);
            let node = string(&t["flow_node"]);
            let display = t["observation"]["type"] == "choices_before_display";
            if !display && t["observation"]["type"] != "choice_selected" {
                return Err("外部システム監視は廃止されました".into());
            }
            let target = if display {
                watch(&event, &pre_choice(&node), &node)
            } else {
                watch(&event, &node, &post_choice(&node))
            };
            let c = condition(
                &r["conditions"],
                &r["external_conditions"],
                "",
                t["observation"]["choice"].as_str(),
            );
            for a in list(&r["actions"]) {
                match a["type"].as_str() {
                    Some("parameter_update") if !display => {
                        operations(&mut interventions, &target, &c, vec![a], "")
                    }
                    Some("add_choice") if display => {
                        let flow = flows
                            .get(owned.get(&event).ok_or("監視先Eventがありません")?)
                            .ok_or("監視先Flowがありません")?;
                        let choice = json!({"id":a["choice"]["id"],"destination":entry(flow,&string(&a["choice"]["next_flow_node"]))});
                        interventions.push(json!({"target":target,"effect":{"condition":c,"effect":{"type":"add_choice","choice":choice}}}));
                        let select =
                            condition(&Value::Null, &Value::Null, "", a["choice"]["id"].as_str());
                        operations(
                            &mut interventions,
                            &watch(&event, &node, &post_choice(&node)),
                            &select,
                            list(&a["on_select_actions"]),
                            "",
                        );
                    }
                    _ => return Err("監視の作用・時点が不正です".into()),
                }
            }
        }
        converted_interceptors
            .push(json!({"id":interceptor["id"],"event":null,"interventions":interventions}));
    }
    // 旧形式に混在する変換済み選択肢も、表示文を実行定義へ残さない。
    for flow in flows.values_mut() {
        for node in flow["flow_nodes"].as_object_mut().unwrap().values_mut() {
            if let Some(choices) = node.get_mut("choices").and_then(Value::as_array_mut) {
                for choice in choices {
                    choice
                        .as_object_mut()
                        .ok_or("選択肢が不正です")?
                        .remove("label");
                }
            }
        }
    }
    for owner in converted_events
        .iter_mut()
        .chain(&mut converted_interceptors)
    {
        for intervention in owner["interventions"].as_array_mut().unwrap() {
            if let Some(choice) = intervention
                .get_mut("effect")
                .and_then(|effect| effect.get_mut("effect"))
                .and_then(|effect| effect.get_mut("choice"))
                .and_then(Value::as_object_mut)
            {
                choice.remove("label");
            }
        }
    }
    root["schema_version"] = json!(3);
    root["flows"] = json!(flows.into_values().collect::<Vec<_>>());
    root["events"] = json!(converted_events);
    root["interceptors"] = json!(converted_interceptors);
    Ok(root)
}

fn entry(flow: &Value, node: &str) -> String {
    let pre = pre_choice(node);
    if flow["flow_nodes"].get(&pre).is_some() {
        pre
    } else {
        node.into()
    }
}

fn connections(flow: &Value) -> Vec<(String, String)> {
    let mut result = BTreeSet::new();
    for (id, node) in flow["flow_nodes"].as_object().unwrap() {
        if let Some(to) = node["next"]["node"].as_str() {
            result.insert((id.clone(), to.into()));
        }
        for c in list(&node["choices"]) {
            result.insert((post_choice(id), string(&c["destination"])));
        }
    }
    result.into_iter().collect()
}
