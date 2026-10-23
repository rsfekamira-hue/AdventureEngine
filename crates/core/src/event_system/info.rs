use super::*;

impl EventSystem {
    pub(super) fn refresh(&mut self) {
        let completed = self.info.status == SystemStatus::Completed;
        let Some((id, node)) = (if completed {
            self.last_visible.clone()
        } else {
            self.current_event
                .map(|e| (e, self.event(e).progress.flow_node.clone()))
        }) else {
            return;
        };
        let same_terminal = completed
            && self.info.event_id == self.event(id).definition.id
            && self.info.flow_node_id == node;
        let stamp = FlowStamp {
            revision: self.info.stamp.revision + u64::from(!same_terminal),
            ..self.info.stamp
        };
        self.gate = if completed {
            ProgressGate::Open
        } else {
            ProgressGate::AwaitingPresentation {
                request_id: stamp.revision,
            }
        };
        self.info = EventSystemInfo {
            stamp,
            status: if completed {
                SystemStatus::Completed
            } else {
                SystemStatus::Active
            },
            event_id: self.event(id).definition.id.clone(),
            flow_id: self.event(id).definition.flow.as_ref().unwrap().id.clone(),
            flow_node_id: node,
            flow_generation: self.event(id).progress.flow_generation,
            choices: if completed {
                Vec::new()
            } else {
                self.choices(id)
                    .into_iter()
                    .map(|c| EventChoice { id: c.id })
                    .collect()
            },
            presentation_request: self.gate.request(),
        };
    }
}
