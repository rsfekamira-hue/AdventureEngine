use serde_json::{json, Value};
use engine_core::event_system::{
    EntryMode, EventInput, EventSystem, JsonHolder, SystemStatus,
};

fn definition() -> Value {
    json!({
        "schema_version": 3,
        "parameter_sets": {"state": {"count":0,"ready":false}},
        "flows": [
            {"id":"main-flow","start_flow_node":"pre","flow_nodes":{
                "pre":{"kind":"auto","next":{"type":"fixed","node":"menu"}},
                "menu":{"kind":"choice","next":{"type":"fixed","node":"post"},"choices":[
                    {"id":"a","destination":"done"},
                    {"id":"b","destination":"done"}
                ]},
                "post":{"kind":"auto","next":{"type":"selected_destination"}},
                "done":{"kind":"advance","next":{"type":"fixed","node":"end"}},
                "end":{"kind":"end"}
            }},
            {"id":"child-flow","start_flow_node":"line","flow_nodes":{
                "line":{"kind":"advance","next":{"type":"fixed","node":"end"}},
                "end":{"kind":"end"}
            }},
            {"id":"other-flow","start_flow_node":"line","flow_nodes":{
                "line":{"kind":"advance","next":{"type":"fixed","node":"end"}},
                "end":{"kind":"end"}
            }}
        ],
        "events":[
            {"id":"main","flow":"main-flow","parameter_set":"state"},
            {"id":"child","flow":"child-flow","parameter_set":"state"},
            {"id":"other","flow":"other-flow","parameter_set":"state"}
        ],
        "interceptors":[]
    })
}

fn target(event: &str, from: &str, to: &str) -> Value {
    json!({"event":event,"from":from,"to":to})
}

fn effect(target: Value, condition: Value, effect: Value) -> Value {
    json!({"target":target,"effect":{"condition":condition,"effect":effect}})
}

fn operation(target: Value, condition: Value, parameter: &str, value: Value) -> Value {
    json!({"target":target,"parameter_operation":{"condition":condition,"operation":{
        "parameter_set":"state","update":{"op":"set","parameter":parameter,"value":value}
    }}})
}

fn load(data: Value) -> EventSystem {
    let mut system = JsonHolder::load(&data.to_string()).unwrap();
    assert!(system.update(EventInput::Start {
        event: system.event_id("main").unwrap(),
        mode: EntryMode::Start
    }));
    system
}

fn ready(system: &mut EventSystem) {
    if let Some(id) = system.info().presentation_request {
        assert!(system.update(EventInput::PresentationReady(id)));
    }
}

fn choose(system: &mut EventSystem, id: &str) {
    ready(system);
    assert!(system.update(EventInput::Choose {
        stamp: system.info().stamp,
        choice: id.into()
    }));
}

fn advance(system: &mut EventSystem) {
    ready(system);
    assert!(system.update(EventInput::Advance {
        stamp: system.info().stamp
    }));
}

#[test]
fn effects_see_all_operations_and_false_operation_does_not_gate_effect() {
    let mut d = definition();
    let t = target("main", "menu", "post");
    let yes = json!({"parameters":[{"parameter_set":"state","comparison":{"parameter":"ready","op":"eq","value":true}}]});
    let mut combined = operation(t.clone(), yes.clone(), "count", json!(99));
    combined["effect"] = json!({"condition":yes,"effect":{"type":"call_event","event":"child"}});
    d["events"][0]["interventions"] = json!([combined]);
    d["interceptors"] = json!([{"id":"update","interventions":[operation(t,json!({"choice":"a"}),"ready",json!(true))]}]);
    let mut s = load(d);
    choose(&mut s, "a");
    assert_eq!(s.info().event_id, "child");
    assert_eq!(s.snapshot()["parameterSets"]["state"]["count"], 0);
    advance(&mut s);
    assert_eq!(s.info().flow_node_id, "done");
    assert_eq!(s.snapshot()["parameterSets"]["state"]["count"], 0);
}

#[test]
fn call_then_switch_preserves_outer_return_and_does_not_repeat_intervention() {
    let mut d = definition();
    let t = target("main", "menu", "post");
    d["interceptors"] = json!([{"id":"route","event":"child","interventions":[
        {"target":t,"parameter_operation":{"condition":{},"operation":{"parameter_set":"state","update":{"op":"add","parameter":"count","value":1}}}},
        effect(t,json!({}),json!({"type":"call_event","event":"child"})),
        effect(target("child","line","end"),json!({}),json!({"type":"switch_event","event":"other"}))
    ]}]);
    let mut s = load(d);
    choose(&mut s, "b");
    advance(&mut s);
    assert_eq!(s.info().event_id, "other");
    advance(&mut s);
    assert_eq!(s.info().event_id, "main");
    assert_eq!(s.info().flow_node_id, "done");
    assert_eq!(s.snapshot()["parameterSets"]["state"]["count"], 1);
    assert_eq!(s.snapshot()["returnPoints"], json!([]));
}

#[test]
fn high_priority_wins_and_end_discards_return_stack() {
    let mut d = definition();
    let t = target("main", "menu", "post");
    d["events"][0]["interventions"] = json!([
        effect(
            t.clone(),
            json!({}),
            json!({"type":"call_event","event":"child","priority":10})
        ),
        effect(
            t,
            json!({}),
            json!({"type":"call_event","event":"other","priority":20})
        )
    ]);
    d["events"][2]["end_behavior"] = json!("end");
    let mut s = load(d);
    choose(&mut s, "a");
    assert_eq!(s.info().event_id, "other");
    advance(&mut s);
    assert_eq!(s.info().status, SystemStatus::Completed);
    assert_eq!(s.snapshot()["returnPoints"], json!([]));
}

#[test]
fn choice_addition_is_built_after_operations_without_mutating_flow_or_replaying() {
    let mut d = definition();
    let t = target("main", "pre", "menu");
    let c = json!({"parameters":[{"parameter_set":"state","comparison":{"parameter":"ready","op":"eq","value":true}}]});
    d["interceptors"] = json!([{"id":"extra","interventions":[
        operation(t.clone(),json!({}),"ready",json!(true)),
        effect(t,c,json!({"type":"add_choice","choice":{"id":"extra","destination":"done"}}))
    ]}]);
    let mut s = load(d);
    assert_eq!(s.info().choices.len(), 3);
    let stale = s.info().stamp;
    choose(&mut s, "extra");
    assert!(s.info().choices.is_empty());
    assert_eq!(s.info().flow_node_id, "done");
    assert!(!s.update(EventInput::Choose {
        stamp: stale,
        choice: "extra".into()
    }));
    s.update(EventInput::Reset);
    assert!(s.update(EventInput::Start {
        event: s.event_id("main").unwrap(),
        mode: EntryMode::Start
    }));
    assert_eq!(s.info().choices.len(), 3);
}

#[test]
fn same_route_distinguishes_choice_ids_and_identical_conditions_are_allowed() {
    let mut d = definition();
    let t = target("main", "menu", "post");
    d["events"][0]["interventions"] = json!([
        operation(t.clone(), json!({"choice":"a"}), "ready", json!(true)),
        operation(t, json!({"choice":"a"}), "count", json!(7))
    ]);
    let mut s = load(d.clone());
    choose(&mut s, "b");
    assert_eq!(s.snapshot()["parameterSets"]["state"]["count"], 0);
    let mut s = load(d);
    choose(&mut s, "a");
    assert_eq!(s.snapshot()["parameterSets"]["state"]["count"], 7);
}

#[test]
fn invalid_references_types_and_removed_external_effect_fail_before_execution() {
    for mutation in [
        json!({"type":"call_event","event":"missing"}),
        json!({"type":"start_external_system","system":"quiz","config":"one"}),
    ] {
        let mut d = definition();
        d["events"][0]["interventions"] =
            json!([effect(target("main", "menu", "post"), json!({}), mutation)]);
        assert!(JsonHolder::load(&d.to_string()).is_err());
    }
    let mut d = definition();
    d["events"][0]["interventions"] = json!([operation(
        target("main", "menu", "missing"),
        json!({}),
        "count",
        json!(true)
    )]);
    assert!(JsonHolder::load(&d.to_string()).is_err());

    let mut d = definition();
    d["events"][1]["interventions"] = json!([operation(
        target("main", "menu", "post"),
        json!({}),
        "count",
        json!(1)
    )]);
    assert!(JsonHolder::load(&d.to_string())
        .err()
        .unwrap()
        .contains("自身のFlow"));
}
