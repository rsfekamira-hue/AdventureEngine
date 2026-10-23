use super::*;
use serde_json::{json, Value};

impl EventSystem {
    pub fn snapshot(&self) -> Value {
        let mut watching = BTreeSet::new();
        let mut monitoring = Vec::new();
        let mut watch_nodes = Vec::new();
        let interceptors:Vec<_>=self.interceptors.entries.iter().map(|i| {
            let rules:Vec<_>=i.interventions.iter().map(|r| {
                let matched=r.target.event==self.info.event_id;
                monitoring.push(json!({"event":i.event.as_ref().unwrap_or(&i.id),"target":r.target.event}));
                if matched { watching.insert(i.event.as_ref().unwrap_or(&i.id).clone()); }
                let condition=r.effect.as_ref().map(|e|&e.condition).or(r.parameter_operation.as_ref().map(|p|&p.condition));
                let ready=condition.is_none_or(|c|self.matches(c,None));
                let row=json!({"event":i.event.as_ref().unwrap_or(&i.id),"kind":"interceptor","target":r.target.event,"flowNode":r.target.from,"to":r.target.to,"matchesNode":matched && (r.target.from==self.info.flow_node_id || (r.target.from.starts_with("@choice:pre:") && r.target.to==self.info.flow_node_id)),"conditionsMet":ready});
                if matched { watch_nodes.push(row.clone()); }
                row
            }).collect();
            json!({"id":i.id,"event":i.event,"rules":rules})
        }).collect();
        let positions:BTreeMap<_,_>=self.events.entries.iter().map(|e|(&e.definition.id,json!({"parameterSet":e.definition.parameter_set,"flow":e.definition.flow.as_ref().map(|f|&f.id),"flowNode":e.progress.flow_node,"visits":e.progress.visits,"canExecute":e.definition.flow.is_some()}))).collect();
        let visits: BTreeMap<_, _> = self
            .events
            .entries
            .iter()
            .filter(|e| e.progress.visits > 0)
            .map(|e| (&e.definition.id, e.progress.visits))
            .collect();
        let dialogue=self.current_event.map(|id|json!({"flowId":self.info.flow_id,"currentFlowNodeId":self.info.flow_node_id,"choiceHistory":self.event(id).progress.choice_history,"currentNode":{"choices":self.info.choices,"nextFlowNode":self.next(id)}})).unwrap_or(json!({}));
        json!({"currentEventId":self.info.event_id,"status":self.info.status,"interceptors":interceptors,"parameterSets":self.parameters.sets,"watcherEvents":watching,"monitoring":monitoring,"watchNodes":watch_nodes,"eventPositions":positions,"eventVisits":visits,"dialogue":dialogue,"returnPoints":self.return_points.iter().map(|p|json!({"event":self.event(p.event).definition.id,"flowNode":p.destination})).collect::<Vec<_>>(),"presentationRequest":self.info.presentation_request})
    }
}
