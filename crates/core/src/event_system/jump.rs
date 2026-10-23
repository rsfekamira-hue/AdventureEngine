use super::*;
use serde_json::{json, Value};
use std::collections::VecDeque;

/// Debug-only operations. Ordinary player input cannot invoke these commands.
pub enum JumpCommand {
    Go {
        event: String,
        node: Option<String>,
    },
    Restore,
    Cancel,
    SetParameter {
        set: String,
        name: String,
        value: ParameterValue,
    },
}
#[derive(Clone, Default, serde::Serialize)]
pub struct JumpReport {
    pub forced: bool,
    pub explored: usize,
    pub steps: usize,
    pub message: String,
    pub issues: Vec<Value>,
    pub route: Vec<String>,
    pub stopped: String,
    pub limited: bool,
}
#[derive(Clone)]
struct Candidate {
    state: EventSystem,
    route: Vec<String>,
    issues: Vec<Value>,
    entered: bool,
}
impl EventSystem {
    /// Graph definitions are exposed only in a debug build, never used as runtime state.
    pub fn jump_graph(&self) -> Value {
        let events: Vec<_> = self
            .events
            .entries
            .iter()
            .filter_map(|e| {
                e.definition
                    .flow
                    .as_ref()
                    .map(|f| json!({"id":e.definition.id,"flow":f}))
            })
            .collect();
        let rules: Vec<_> = self
            .events
            .entries
            .iter()
            .flat_map(|e| &e.definition.interventions)
            .chain(
                self.interceptors
                    .entries
                    .iter()
                    .flat_map(|i| &i.interventions),
            )
            .collect();
        json!({"events":events,"rules":rules})
    }
    pub fn debug_parameters(&self) -> Value {
        json!(self.parameters.sets)
    }
    pub fn debug_conditions(&self) -> Value {
        json!(self
            .condition_names
            .iter()
            .map(|name| (name, self.facts.get(name)))
            .collect::<BTreeMap<_, _>>())
    }
    pub fn debug_parameter(
        &mut self,
        set: &str,
        name: String,
        value: ParameterValue,
    ) -> Result<(), String> {
        self.parameters
            .sets
            .get_mut(set)
            .ok_or("ParameterSetがありません")?
            .apply(&[ParameterUpdate::Set {
                parameter: name,
                value,
            }])?;
        Ok(())
    }
    fn jump_key(&self) -> String {
        // Visits count and history do not affect Core conditions; resume availability does.
        json!([
            self.info.status,
            self.current_event.map(|id| id.index()),
            self.parameters.sets,
            self.events
                .entries
                .iter()
                .map(|e| json!([
                    e.progress.flow_node,
                    e.progress.selected,
                    e.progress.added_choices,
                    e.progress.visits > 0,
                    e.progress.finished
                ]))
                .collect::<Vec<_>>(),
            self.return_points
                .iter()
                .map(|p| json!([p.event.index(), p.destination, p.selected, p.added_choices]))
                .collect::<Vec<_>>()
        ])
        .to_string()
    }
    fn jump_issues(&self, id: EventId, to: &str) -> Vec<Value> {
        let e = self.event(id);
        let selected = if self.node(id).kind == FlowNodeKind::Choice {
            e.progress.selected.as_ref().map(|c| c.id.as_str())
        } else {
            None
        };
        let rules: Vec<_> = e
            .definition
            .interventions
            .iter()
            .chain(
                self.interceptors
                    .entries
                    .iter()
                    .flat_map(|i| &i.interventions),
            )
            .filter(|r| {
                r.target.event == e.definition.id
                    && r.target.from == e.progress.flow_node
                    && r.target.to == to
            })
            .collect();
        let mut post = self.clone();
        for op in rules
            .iter()
            .filter_map(|r| r.parameter_operation.as_ref())
            .filter(|p| self.matches(&p.condition, selected))
        {
            post.parameters
                .sets
                .get_mut(&op.operation.parameter_set)
                .unwrap()
                .apply(std::slice::from_ref(&op.operation.update))
                .expect("validated update");
        }
        let mut issues = Vec::new();
        let mut requested = rules
            .iter()
            .filter_map(|r| r.effect.as_ref())
            .filter(|e| post.matches(&e.condition, selected))
            .filter_map(|e| match &e.effect {
                Effect::CallEvent {
                    event,
                    priority,
                    entry_mode,
                }
                | Effect::SwitchEvent {
                    event,
                    priority,
                    entry_mode,
                } => {
                    let target = post.event(post.event_id(event).unwrap());
                    if *entry_mode == EntryMode::Resume
                        && (target.progress.visits == 0 || target.progress.finished)
                    {
                        None
                    } else {
                        Some((*priority, event))
                    }
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        requested.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        for loser in requested.iter().skip(1) {
            issues.push(json!({"event":e.definition.id,"from":e.progress.flow_node,"to":to,"name":"介入の優先順位",
                "expected":loser.1,"actual":requested[0].1,"op":"priority","priority":loser.0,"selectedPriority":requested[0].0}));
        }
        for r in rules {
            let conditions = r
                .parameter_operation
                .as_ref()
                .map(|p| (&p.condition, self, "更新前"))
                .into_iter()
                .chain(r.effect.as_ref().map(|e| (&e.condition, &post, "更新後")));
            for (c, state, phase) in conditions.filter(|(c, _, _)| {
                c.choice
                    .as_deref()
                    .is_none_or(|choice| Some(choice) == selected)
            }) {
                for p in &c.parameters {
                    if !state.parameters.sets[&p.parameter_set]
                        .matches(std::slice::from_ref(&p.comparison))
                    {
                        issues.push(json!({"event":e.definition.id,"from":e.progress.flow_node,"to":to,"phase":phase,
                            "set":p.parameter_set,"name":p.comparison.parameter,"op":p.comparison.op,"expected":p.comparison.value,
                            "actual":state.parameter_set_value(&p.parameter_set,&p.comparison.parameter)}));
                    }
                }
                for p in &c.external {
                    if !p.matches(&state.facts) {
                        issues.push(json!({"event":e.definition.id,"from":e.progress.flow_node,"to":to,"phase":phase,
                        "external":p.name,"op":"eq","expected":p.equals,"actual":state.facts.get(&p.name)}));
                    }
                }
            }
            if let Some(effect) = &r.effect {
                if post.matches(&effect.condition, selected) {
                    match &effect.effect {
                        Effect::CallEvent {
                            event,
                            entry_mode: EntryMode::Resume,
                            ..
                        }
                        | Effect::SwitchEvent {
                            event,
                            entry_mode: EntryMode::Resume,
                            ..
                        } => {
                            let target = post.event(post.event_id(event).unwrap());
                            if target.progress.visits == 0 || target.progress.finished {
                                issues.push(json!({"event":e.definition.id,"from":e.progress.flow_node,"to":to,"name":"再開不可","expected":event,"actual":"未開始または終了済み"}));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        issues
    }
    /// Execute one logical edge without a frame, presentation wait, or AUTO timer.
    /// Operations, effect priority, call/return and choice additions use the production engine.
    fn jump_step(&mut self, choice: Option<FlowChoice>) -> Vec<Value> {
        let Some(id) = self.current_event else {
            return Vec::new();
        };
        if self.node(id).kind == FlowNodeKind::End {
            self.finish_event(id);
            return Vec::new();
        }
        if let Some(choice) = choice {
            let p = &mut self.event_mut(id).progress;
            p.choice_history
                .push(json!({"flowNode":p.flow_node,"choice":choice.id}));
            p.selected = Some(choice);
        }
        if let Some(to) = self.next(id) {
            let issues = self.jump_issues(id, &to);
            self.transition(id, to);
            issues
        } else {
            Vec::new()
        }
    }
    pub fn start_debug_jump(&self, event: &str, node: Option<&str>) -> Result<JumpSearch, String> {
        let target = self.event_id(event).ok_or("イベントがありません")?;
        let flow = self
            .event(target)
            .definition
            .flow
            .as_ref()
            .ok_or("監視専用イベントには移動できません")?;
        if !flow
            .flow_nodes
            .values()
            .any(|n| !n.generated && matches!(n.kind, FlowNodeKind::Advance | FlowNodeKind::Choice))
        {
            return Err("表示可能なノードがないイベントです".into());
        }
        let destination = node.unwrap_or(&flow.start_flow_node).to_owned();
        let target_node = flow
            .flow_nodes
            .get(&destination)
            .ok_or("フロウノードがありません")?;
        if node.is_some()
            && (target_node.generated
                || !matches!(
                    target_node.kind,
                    FlowNodeKind::Advance | FlowNodeKind::Choice
                ))
        {
            return Err("表示可能なフロウノードを選択してください".into());
        }
        // Reverse structural distances guide bounded search; only real transitions determine success.
        let mut edges: Vec<((EventId, String), (EventId, String))> = Vec::new();
        for (i, e) in self.events.entries.iter().enumerate() {
            let id = EventId::from_index(i);
            if let Some(f) = &e.definition.flow {
                for (name, n) in &f.flow_nodes {
                    if let Some(NextNode::Fixed { node }) = &n.next {
                        edges.push(((id, name.clone()), (id, node.clone())));
                    }
                    for c in &n.choices {
                        edges.push(((id, name.clone()), (id, c.destination.clone())));
                        // SelectedDestination is evaluated after generated post-choice nodes.
                        if let Some(NextNode::Fixed { node }) = &n.next {
                            edges.push(((id, node.clone()), (id, c.destination.clone())));
                        }
                    }
                }
            }
        }
        for r in self
            .events
            .entries
            .iter()
            .flat_map(|e| &e.definition.interventions)
            .chain(
                self.interceptors
                    .entries
                    .iter()
                    .flat_map(|i| &i.interventions),
            )
        {
            let Some(from) = self.event_id(&r.target.event) else {
                continue;
            };
            if let Some(effect) = &r.effect {
                match &effect.effect {
                    Effect::CallEvent { event, .. } | Effect::SwitchEvent { event, .. } => {
                        if let Some(to) = self.event_id(event) {
                            if let Some(f) = &self.event(to).definition.flow {
                                edges.push((
                                    (from, r.target.from.clone()),
                                    (to, f.start_flow_node.clone()),
                                ));
                            }
                        }
                    }
                    Effect::AddChoice { choice } => edges.push((
                        (from, r.target.to.clone()),
                        (from, choice.destination.clone()),
                    )),
                }
            }
        }
        let mut distances = BTreeMap::new();
        let mut pending = VecDeque::from([((target, destination.clone()), 0usize)]);
        while let Some((position, distance)) = pending.pop_front() {
            if distances.contains_key(&position) {
                continue;
            }
            distances.insert(position.clone(), distance);
            for (from, to) in &edges {
                if *to == position {
                    pending.push_back((from.clone(), distance + 1));
                }
            }
        }
        let initial = Candidate {
            state: self.clone(),
            route: Vec::new(),
            issues: Vec::new(),
            entered: self.current_event == Some(target)
                && self.event(target).progress.flow_node == destination,
        };
        Ok(JumpSearch {
            target,
            destination,
            event_entry: node.is_none(),
            distances,
            queue: vec![initial.clone()],
            best: initial,
            seen: BTreeSet::new(),
            explored: 0,
            limited: false,
        })
    }
    pub fn adopt_debug_jump(&mut self, mut state: EventSystem) {
        state.info.stamp.generation = self.info.stamp.generation + 1;
        state.info.stamp.revision = self.info.stamp.revision + 1;
        state.gate = ProgressGate::Open;
        state.last_visible = state
            .current_event
            .map(|id| (id, state.event(id).progress.flow_node.clone()));
        state.refresh();
        // Session already blocks input while its new scene is busy. Do not issue
        // PresentationReady here: terminal events can run departure effects on
        // that notification, consuming the very node requested by this jump.
        state.gate = ProgressGate::Open;
        state.info.presentation_request = None;
        *self = state;
    }
    pub fn debug_restore(&mut self, mut saved: EventSystem) {
        saved.info.stamp.generation = self.info.stamp.generation + 1;
        saved.info.stamp.revision = self.info.stamp.revision + 1;
        saved.refresh();
        *self = saved;
    }
}

/// Speculative progress remains separate from the live session until adoption.
pub struct JumpSearch {
    target: EventId,
    destination: String,
    event_entry: bool,
    distances: BTreeMap<(EventId, String), usize>,
    queue: Vec<Candidate>,
    best: Candidate,
    seen: BTreeSet<String>,
    pub explored: usize,
    limited: bool,
}
impl JumpSearch {
    fn score(&self, c: &Candidate) -> usize {
        c.state
            .current_event
            .and_then(|id| {
                self.distances
                    .get(&(id, c.state.event(id).progress.flow_node.clone()))
                    .copied()
            })
            .unwrap_or(usize::MAX / 2)
    }
    fn reached(&self, c: &Candidate) -> bool {
        if self.event_entry {
            c.entered
                && c.state.current_event.is_some_and(|id| {
                    matches!(
                        c.state.node(id).kind,
                        FlowNodeKind::Advance | FlowNodeKind::Choice
                    )
                })
        } else {
            c.state.current_event == Some(self.target)
                && c.state.event(self.target).progress.flow_node == self.destination
        }
    }
    pub fn advance(&mut self, budget: usize) -> Option<(EventSystem, JumpReport)> {
        for _ in 0..budget {
            if self.queue.is_empty() || self.explored >= 2048 {
                return Some(self.finish(None));
            }
            let index = self
                .queue
                .iter()
                .enumerate()
                .min_by_key(|(_, c)| self.score(c).saturating_add(c.route.len() / 8))
                .unwrap()
                .0;
            let candidate = self.queue.swap_remove(index);
            if self.reached(&candidate) {
                return Some(self.finish(Some(candidate)));
            }
            if candidate.route.len() >= 512 {
                self.limited = true;
                continue;
            }
            if !self.seen.insert(format!(
                "{}:{}",
                candidate.entered,
                candidate.state.jump_key()
            )) {
                continue;
            }
            self.explored += 1;
            if self.score(&candidate) < self.score(&self.best) {
                self.best = candidate.clone();
            }
            let Some(id) = candidate.state.current_event else {
                continue;
            };
            if candidate.state.info.status == SystemStatus::Completed
                || !candidate.state.conditions_ready()
            {
                continue;
            }
            let choices = if candidate.state.node(id).kind == FlowNodeKind::Choice {
                candidate
                    .state
                    .choices(id)
                    .into_iter()
                    .map(Some)
                    .collect::<Vec<_>>()
            } else {
                vec![None]
            };
            for choice in choices {
                let mut next = candidate.clone();
                next.route.push(format!(
                    "{} / {}{}",
                    next.state.event(id).definition.id,
                    next.state.event(id).progress.flow_node,
                    choice
                        .as_ref()
                        .map(|c| format!(" → {}", c.id))
                        .unwrap_or_default()
                ));
                let issues = next.state.jump_step(choice);
                if next.state.current_event == Some(self.target)
                    && next.state.event(self.target).progress.flow_node == self.destination
                {
                    next.entered = true;
                }
                for issue in issues {
                    if next.issues.len() < 256 && !next.issues.contains(&issue) {
                        next.issues.push(issue);
                    }
                }
                if self.queue.len() < 128 {
                    self.queue.push(next);
                } else {
                    self.limited = true;
                }
            }
        }
        None
    }
    fn finish(&mut self, reached: Option<Candidate>) -> (EventSystem, JumpReport) {
        let forced = reached.is_none();
        self.limited |= self.explored >= 2048;
        let mut result = reached.unwrap_or_else(|| self.best.clone());
        let mut message = if forced {
            "候補経路から強制移動。条件は任意分岐を含み、必須条件の完全な一覧ではありません。"
        } else {
            "通常の遷移処理を高速実行して到達。"
        }
        .to_owned();
        if forced {
            for name in &result.state.condition_names {
                if result.state.facts.get(name).is_none() {
                    result.issues.push(json!({"event":event_name(&result.state),"external":name,"name":name,"actual":null,"expected":"外部条件の提供","op":"provided"}));
                }
            }
            if let Some(id) = result.state.current_event {
                if let Some(to) = result.state.next(id) {
                    for issue in result.state.jump_issues(id, &to) {
                        if !result.issues.contains(&issue) {
                            result.issues.push(issue);
                        }
                    }
                }
                if result.state.node(id).kind == FlowNodeKind::Choice
                    && result.state.choices(id).is_empty()
                {
                    result.issues.push(json!({"event":result.state.event(id).definition.id,"from":result.state.event(id).progress.flow_node,"name":"選択肢不足","actual":"有効な選択肢なし"}));
                }
            }
            result.state.return_points.clear();
            result.state.begin(self.target, EntryMode::Start);
            if self.event_entry {
                let mut visited = BTreeSet::new();
                while let Some(id) = result.state.current_event {
                    if !visited.insert((id, result.state.event(id).progress.flow_node.clone())) {
                        message.push_str(" 開始処理が循環しました。");
                        break;
                    }
                    if matches!(
                        result.state.node(id).kind,
                        FlowNodeKind::Advance | FlowNodeKind::Choice
                    ) {
                        break;
                    }
                    if result.state.info.status == SystemStatus::Completed {
                        break;
                    }
                    let issues = result.state.jump_step(None);
                    result.issues.extend(issues);
                }
                if result.state.current_event.is_none_or(|id| {
                    !matches!(
                        result.state.node(id).kind,
                        FlowNodeKind::Advance | FlowNodeKind::Choice
                    )
                }) {
                    // A cyclic or completed entry cannot supply a display. Choose a visible
                    // target explicitly rather than publishing an internal generated node.
                    let visible = result
                        .state
                        .event(self.target)
                        .definition
                        .flow
                        .as_ref()
                        .unwrap()
                        .flow_nodes
                        .iter()
                        .find(|(_, n)| {
                            !n.generated
                                && matches!(n.kind, FlowNodeKind::Advance | FlowNodeKind::Choice)
                        })
                        .unwrap()
                        .0
                        .clone();
                    result.state.return_points.clear();
                    result.state.begin(self.target, EntryMode::Resume);
                    let progress = &mut result.state.event_mut(self.target).progress;
                    progress.flow_node = visible;
                    progress.finished = false;
                    progress.selected = None;
                    progress.added_choices.clear();
                    message.push_str(
                        " 開始処理が表示地点に達しないため、表示可能ノードへ強制移動しました。",
                    );
                }
                if result.state.current_event != Some(self.target) {
                    message.push_str(" 開始時の介入により別イベントへ停止しました。");
                }
            } else {
                result.state.event_mut(self.target).progress.flow_node = self.destination.clone();
            }
        }
        if !forced && self.event_entry && result.state.current_event != Some(self.target) {
            message.push_str(" 開始時の介入により別イベントへ停止しました。");
        }
        if forced {
            result.issues.push(json!({"event":event_name(&result.state),"name":if self.limited {"探索上限"} else {"経路不足"},"actual":if self.limited {"探索範囲内で未到達。到達不能の証明ではありません"} else {"有効な未探索候補がなくなりました"}}));
        }
        let stopped = result
            .state
            .current_event
            .map(|id| {
                format!(
                    "{} / {}",
                    result.state.event(id).definition.id,
                    result.state.event(id).progress.flow_node
                )
            })
            .unwrap_or_default();
        let report = JumpReport {
            forced,
            explored: self.explored,
            steps: result.route.len(),
            message,
            issues: result.issues,
            route: result.route,
            stopped,
            limited: self.limited,
        };
        (result.state, report)
    }
}

fn event_name(state: &EventSystem) -> &str {
    state
        .current_event
        .map(|id| state.event(id).definition.id.as_str())
        .unwrap_or("")
}
