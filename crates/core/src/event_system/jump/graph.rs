use super::*;

type Edge = (String, String, String);

impl EventSystem {
    /// Static graph for debug inspectors. An empty jumpNodes list means an
    /// event has no jump destination. Edges describe possible connections
    /// from definitions, independent of the currently satisfied conditions.
    pub fn jump_graph(&self) -> Value {
        let rules: Vec<_> = self.debug_interventions().collect();
        let event_edges: Vec<_> = rules
            .iter()
            .filter_map(|rule| {
                let (event, kind) = event_transfer(rule)?;
                Some((&rule.target.event, event, kind))
            })
            .collect();
        let events: Vec<_> = self
            .events
            .entries
            .iter()
            .filter_map(|event| {
                let id = &event.definition.id;
                let flow = event.definition.flow.as_ref()?;
                let nodes: Vec<_> = flow
                    .flow_nodes
                    .iter()
                    .filter(|(_, node)| is_jump_node(node))
                    .map(|(id, _)| id.as_str())
                    .collect();
                // Start graph layout at the entrance even when the flow has cycles.
                let entrance = nodes
                    .iter()
                    .find(|id| generated_connection(flow, &flow.start_flow_node, id));
                let edges = flow_edges(flow, &nodes, &rules, id);
                Some(json!({"id":id,"flow":flow,"jumpNodes":nodes,"entrance":entrance,"edges":edges}))
            })
            .collect();
        json!({"events":events,"rules":rules,"eventEdges":event_edges})
    }

    pub(super) fn debug_interventions(&self) -> impl Iterator<Item = &Intervention> {
        self.events
            .entries
            .iter()
            .flat_map(|event| &event.definition.interventions)
            .chain(
                self.interceptors
                    .entries
                    .iter()
                    .flat_map(|interceptor| &interceptor.interventions),
            )
    }
}

fn successors<'a>(flow: &'a Flow, id: &str) -> impl Iterator<Item = (&'a str, &'a str)> {
    flow.flow_nodes.get(id).into_iter().flat_map(|node| {
        let next = match &node.next {
            Some(NextNode::Fixed { node }) => Some((node.as_str(), "")),
            _ => None,
        };
        next.into_iter().chain(
            node.choices
                .iter()
                .map(|choice| (choice.destination.as_str(), choice.id.as_str())),
        )
    })
}

fn event_transfer(rule: &Intervention) -> Option<(&str, &'static str)> {
    match &rule.effect.as_ref()?.effect {
        Effect::CallEvent { event, .. } => Some((event, "call_event")),
        Effect::SwitchEvent { event, .. } => Some((event, "switch_event")),
        Effect::AddChoice { .. } => None,
    }
}

fn flow_edges(flow: &Flow, nodes: &[&str], rules: &[&Intervention], event: &str) -> Vec<Edge> {
    let visible: BTreeSet<_> = nodes.iter().copied().collect();
    let mut edges = Vec::new();
    for &from in nodes {
        let mut pending: Vec<_> = successors(flow, from)
            .map(|(to, label)| (to, label.to_owned()))
            .collect();
        // Authored and added choices can both point through generated nodes.
        for rule in rules
            .iter()
            .filter(|rule| rule.target.event == event && rule.target.to == from)
        {
            if let Some(ConditionalEffect {
                effect: Effect::AddChoice { choice },
                ..
            }) = &rule.effect
            {
                pending.push((choice.destination.as_str(), format!("+ {}", choice.id)));
            }
        }
        let mut seen = BTreeSet::new();
        while let Some((to, label)) = pending.pop() {
            if !seen.insert((to, label.clone())) {
                continue;
            }
            if visible.contains(to) {
                edges.push((from.to_owned(), to.to_owned(), label));
                continue;
            }
            pending.extend(successors(flow, to).map(|(next, branch)| {
                let label = if branch.is_empty() {
                    label.as_str()
                } else {
                    branch
                };
                (next, label.to_owned())
            }));
        }
    }
    // Keep references to other events visible even when internal nodes are folded.
    for rule in rules {
        let Some((target, kind)) = event_transfer(rule) else {
            continue;
        };
        if rule.target.event == event {
            for &from in nodes {
                if generated_connection(flow, from, &rule.target.from) {
                    edges.push((from.to_owned(), format!("→ {target}"), kind.into()));
                }
            }
        }
        if target == event && flow.flow_nodes.contains_key(PRE_EVENT) {
            for &to in nodes {
                if generated_connection(flow, PRE_EVENT, to) {
                    edges.push((
                        format!("← {}", rule.target.event),
                        to.to_owned(),
                        kind.into(),
                    ));
                }
            }
        }
    }
    edges.sort();
    edges.dedup();
    edges
}

/// Traverse internal edges while stopping at the next displayed node.
fn generated_connection<'a>(flow: &'a Flow, from: &'a str, to: &str) -> bool {
    let mut pending = vec![from];
    let mut seen = BTreeSet::new();
    let generated = |id| flow.flow_nodes.get(id).is_some_and(|node| node.generated);
    while let Some(id) = pending.pop() {
        if id == to {
            return id == from || generated(id) || generated(from);
        }
        if seen.insert(id) && (id == from || generated(id)) {
            pending.extend(successors(flow, id).map(|(next, _)| next));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_graph_folds_generated_nodes_and_lists_only_valid_targets() {
        let definition = json!({
            "schema_version":3,
            "parameter_sets":{"p":{}},
            "flows":[
                {"id":"f","start_flow_node":"@event:start","flow_nodes":{
                    "@event:start":{"kind":"auto","generated":true,"next":{"type":"fixed","node":"room"}},
                    "room":{"kind":"choice","next":{"type":"fixed","node":"post"},"choices":[{"id":"forward","destination":"cyber"}]},
                    "post":{"kind":"auto","generated":true,"next":{"type":"selected_destination"}},
                    "@pre:cyber":{"kind":"auto","generated":true,"next":{"type":"fixed","node":"cyber"}},
                    "cyber":{"kind":"advance","next":{"type":"fixed","node":"room"}}
                }},
                {"id":"silent","start_flow_node":"end","flow_nodes":{"end":{"kind":"end"}}}
            ],
            "events":[
                {"id":"journey","flow":"f","parameter_set":"p"},
                {"id":"silent","flow":"silent","parameter_set":"p"},
                {"id":"watcher","parameter_set":"p"}
            ],
            "interceptors":[{"id":"extra","interventions":[{
                "target":{"event":"journey","from":"@event:start","to":"room"},
                "effect":{"condition":{},"effect":{"type":"add_choice","choice":{"id":"dive","destination":"@pre:cyber"}}}
            }]}]
        });
        let system = JsonHolder::load(&definition.to_string()).unwrap();
        let graph = system.jump_graph();
        let journey = &graph["events"][0];
        assert_eq!(journey["jumpNodes"], json!(["cyber", "room"]));
        assert_eq!(journey["entrance"], "room");
        assert_eq!(
            journey["edges"],
            json!([
                ["cyber", "room", ""],
                ["room", "cyber", "+ dive"],
                ["room", "cyber", "forward"]
            ])
        );
        assert_eq!(graph["events"][1]["jumpNodes"], json!([]));
        assert_eq!(graph["events"].as_array().unwrap().len(), 2);
        for node in ["room", "cyber"] {
            assert!(system
                .validate_debug_jump_target("journey", Some(node))
                .is_ok());
        }
        assert!(system.validate_debug_jump_target("journey", None).is_ok());
        assert!(system
            .validate_debug_jump_target("journey", Some("@pre:cyber"))
            .is_err());
        assert!(system.validate_debug_jump_target("silent", None).is_err());
        assert!(system.validate_debug_jump_target("watcher", None).is_err());
    }
}
