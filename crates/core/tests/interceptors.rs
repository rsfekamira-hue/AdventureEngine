use serde_json::{json, Value};
use engine_core::event_system::{
    EntryMode, EventInput, EventSystem, JsonHolder, ParameterValue,
};

fn definition() -> Value {
    json!({"schema_version":2,
    "parameter_sets":{"root":{},"player":{"vigilance":20},"interest":{"count":0}},
    "flows":[{"id":"station-flow","start_flow_node":"guide","flow_nodes":{
        "guide":{"choices":[{"id":"accept","label":"受け入れる","next_flow_node":"done"}]},"done":{}}}],
    "events":[{"id":"station","flow":"station-flow","parameter_set":"root"}],
    "interceptors":[{"id":"guide","rules":[{
        "id":"interest","trigger":{"event":"station","flow_node":"guide","observation":{"type":"choice_selected","choice":"accept"}},
        "conditions":[{"parameter_set":"player","parameter":"vigilance","op":"lt","value":30}],
        "actions":[{"type":"parameter_update","parameter_set":"interest","op":"add","parameter":"count","value":10}]
    }]}]})
}
fn start(system: &mut EventSystem) {
    assert!(system.update(EventInput::Start {
        event: system.event_id("station").unwrap(),
        mode: EntryMode::Start
    }));
}
fn choose(system: &mut EventSystem, choice: &str) {
    if let Some(request) = system.info().presentation_request {
        assert!(system.update(EventInput::PresentationReady(request)));
    }
    assert!(system.update(EventInput::Choose {
        stamp: system.info().stamp,
        choice: choice.into()
    }));
}

#[test]
fn independent_interceptor_reads_player_updates_another_set_and_reset_restores_it() {
    let mut system = JsonHolder::load(&definition().to_string()).unwrap();
    assert!(system.event_id("guide").is_none());
    start(&mut system);
    choose(&mut system, "accept");
    assert_eq!(
        system.parameter_set_value("interest", "count"),
        Some(&ParameterValue::Int(10))
    );
    assert!(system.update(EventInput::Reset));
    assert_eq!(
        system.parameter_set_value("interest", "count"),
        Some(&ParameterValue::Int(0))
    );
}

#[test]
fn operation_conditions_use_the_pre_operation_snapshot() {
    let mut data = definition();
    data["parameter_sets"]["player"]["vigilance"] = json!(40);
    let mut system = JsonHolder::load(&data.to_string()).unwrap();
    start(&mut system);
    choose(&mut system, "accept");
    assert_eq!(
        system.parameter_set_value("interest", "count"),
        Some(&ParameterValue::Int(0))
    );
    data["parameter_sets"]["player"]["vigilance"] = json!(20);
    let mut rule = data["interceptors"][0]["rules"][0].clone();
    rule["id"] = json!("follow");
    rule["conditions"] =
        json!([{"parameter_set":"interest","parameter":"count","op":"eq","value":10}]);
    data["interceptors"][0]["rules"]
        .as_array_mut()
        .unwrap()
        .push(rule);
    let mut system = JsonHolder::load(&data.to_string()).unwrap();
    start(&mut system);
    choose(&mut system, "accept");
    assert_eq!(
        system.parameter_set_value("interest", "count"),
        Some(&ParameterValue::Int(10))
    );
}

#[test]
fn injected_choices_own_their_actions_and_can_be_observed_before_their_definition() {
    let mut data = definition();
    data["interceptors"][0]["rules"][0]["trigger"]["observation"]["choice"] = json!("special");
    data["interceptors"].as_array_mut().unwrap().push(json!({"id":"menu","rules":[{
        "id":"offer","trigger":{"event":"station","flow_node":"guide","observation":{"type":"choices_before_display"}},
        "actions":[{"type":"add_choice","choice":{"id":"special","label":"特別な案内","next_flow_node":"done"},
            "on_select_actions":[{"type":"parameter_update","parameter_set":"player","op":"set","parameter":"vigilance","value":10}]}]
    }]}));
    let mut system = JsonHolder::load(&data.to_string()).unwrap();
    start(&mut system);
    assert_eq!(system.info().choices.len(), 2);
    assert_eq!(
        system.parameter_set_value("player", "vigilance"),
        Some(&ParameterValue::Int(20))
    );
    choose(&mut system, "special");
    assert_eq!(
        system.parameter_set_value("player", "vigilance"),
        Some(&ParameterValue::Int(10))
    );
    assert_eq!(
        system.parameter_set_value("interest", "count"),
        Some(&ParameterValue::Int(10))
    );
}

#[test]
fn invalid_references_types_duplicates_and_display_updates_fail_at_load() {
    let baseline = definition();
    let mut variants = Vec::new();
    let mut d = baseline.clone();
    d["interceptors"][0]["rules"][0]["trigger"]["event"] = json!("missing");
    variants.push(d);
    let mut d = baseline.clone();
    d["interceptors"][0]["rules"][0]["trigger"]["flow_node"] = json!("missing");
    variants.push(d);
    let mut d = baseline.clone();
    d["interceptors"][0]["rules"][0]["trigger"]["observation"]["choice"] = json!("missing");
    variants.push(d);
    let mut d = baseline.clone();
    d["interceptors"][0]["rules"][0]["conditions"][0]["value"] = json!(true);
    variants.push(d);
    let mut d = baseline.clone();
    d["interceptors"][0]["rules"][0]["actions"][0]["parameter_set"] = json!("missing");
    variants.push(d);
    let mut d = baseline.clone();
    d["interceptors"][0]["rules"][0]["actions"][0]["parameter"] = json!("missing");
    variants.push(d);
    let mut d = baseline.clone();
    d["interceptors"][0]["rules"][0]["actions"][0]["op"] = json!("set");
    d["interceptors"][0]["rules"][0]["actions"][0]["value"] = json!(false);
    variants.push(d);
    let mut d = baseline.clone();
    let i = d["interceptors"][0].clone();
    d["interceptors"].as_array_mut().unwrap().push(i);
    variants.push(d);
    let mut d = baseline.clone();
    let r = d["interceptors"][0]["rules"][0].clone();
    d["interceptors"][0]["rules"]
        .as_array_mut()
        .unwrap()
        .push(r);
    variants.push(d);
    let mut d = baseline.clone();
    d["interceptors"][0]["rules"][0]["trigger"]["observation"] =
        json!({"type":"choices_before_display"});
    variants.push(d);
    let mut d = baseline;
    d["interceptors"][0]["rules"][0]["actions"][0]["typo"] = json!(1);
    variants.push(d);
    for (index, d) in variants.iter().enumerate() {
        assert!(JsonHolder::load(&d.to_string()).is_err(), "variant {index}");
    }
}
