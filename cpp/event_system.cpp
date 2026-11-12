#include "event_system.h"

#include <algorithm>
#include <limits>
#include <stdexcept>
#include <utility>

namespace adventure {
namespace {

constexpr auto post_event = L"@event:end";

bool matches_value(const Value& actual, Comparison comparison, const Value& expected) {
    switch (comparison) {
    case Comparison::eq: return actual == expected;
    case Comparison::ne: return actual != expected;
    case Comparison::gt: return actual > expected;
    case Comparison::gte: return actual >= expected;
    case Comparison::lt: return actual < expected;
    case Comparison::lte: return actual <= expected;
    }
    throw std::logic_error("Unknown comparison");
}

void apply(Parameters& parameters, const ParameterOperation& operation) {
    auto& value = parameters.at(operation.set).at(operation.parameter);
    if (operation.update == Update::set) {
        value = operation.value;
    } else {
        const int64_t sum = int64_t{std::get<int32_t>(value)} + std::get<int32_t>(operation.value);
        value = static_cast<int32_t>(std::clamp(sum,
            int64_t{std::numeric_limits<int32_t>::min()}, int64_t{std::numeric_limits<int32_t>::max()}));
    }
}

}

EventSystem::EventSystem(Definition definition)
    : definition_(std::move(definition)), parameters_(definition_.parameters) {
    validate();
    for (const auto& [id, event] : definition_.events) {
        progress_.emplace(id, Progress{});
        for (const auto& rule : event.interventions) {
            rules_[rule.edge].push_back(&rule);
        }
    }
    for (const auto& rule : definition_.interceptors) {
        rules_[rule.edge].push_back(&rule);
    }
}

void EventSystem::validate() const {
    for (const auto& [flow_id, flow] : definition_.flows) {
        if (flow_id.empty() || !flow.nodes.contains(flow.start)) {
            throw std::invalid_argument("Invalid flow start");
        }
        for (const auto& [node_id, node] : flow.nodes) {
            if (node_id.empty()) {
                throw std::invalid_argument("Empty flow node ID");
            }
            if (const auto* target = std::get_if<Id>(&node.next); target && !flow.nodes.contains(*target)) {
                throw std::invalid_argument("Unknown flow destination");
            }
            std::set<Id> choice_ids;
            for (const auto& choice : node.choices) {
                if (node.kind != NodeKind::choice || choice.id.empty() ||
                    !choice_ids.insert(choice.id).second || !flow.nodes.contains(choice.destination)) {
                    throw std::invalid_argument("Invalid flow choice");
                }
            }
        }
    }
    for (const auto& [event_id, event] : definition_.events) {
        if (event_id.empty() || !definition_.parameters.contains(event.parameter_set) ||
            (event.flow && !definition_.flows.contains(*event.flow))) {
            throw std::invalid_argument("Invalid event definition");
        }
    }
    const auto validate_condition = [&](const Condition& condition) {
        for (const auto& comparison : condition.parameters) {
            const auto& actual = definition_.parameters.at(comparison.set).at(comparison.parameter);
            const bool equality = comparison.comparison == Comparison::eq || comparison.comparison == Comparison::ne;
            if (actual.index() != comparison.value.index() || (!equality && !std::holds_alternative<int32_t>(actual))) {
                throw std::invalid_argument("Invalid parameter comparison type");
            }
        }
        for (const auto& [id, expected] : condition.external) {
            if (!definition_.external_conditions.contains(id)) {
                throw std::invalid_argument("Unknown external condition");
            }
        }
    };
    const auto validate_rule = [&](const Intervention& rule) {
        const auto& target_flow = flow(rule.edge.event);
        const auto destination = target_flow.nodes.find(rule.edge.to);
        if (!target_flow.nodes.contains(rule.edge.from) || destination == target_flow.nodes.end()) {
            throw std::invalid_argument("Invalid intervention edge");
        }
        if (rule.parameter) {
            const auto& action = rule.parameter->action;
            const auto& initial = definition_.parameters.at(action.set).at(action.parameter);
            if (initial.index() != action.value.index() ||
                (action.update == Update::add && !std::holds_alternative<int32_t>(initial))) {
                throw std::invalid_argument("Invalid parameter update type");
            }
            validate_condition(rule.parameter->condition);
        }
        if (rule.effect) {
            validate_condition(rule.effect->condition);
            if (const auto* choice = std::get_if<Choice>(&rule.effect->action)) {
                if (choice->id.empty() || destination->second.kind != NodeKind::choice ||
                    !target_flow.nodes.contains(choice->destination)) {
                    throw std::invalid_argument("Invalid added choice");
                }
            } else {
                flow(std::get<EventTransfer>(rule.effect->action).event);
            }
        }
    };
    for (const auto& [id, event] : definition_.events) {
        for (const auto& rule : event.interventions) {
            if (rule.edge.event != id) {
                throw std::invalid_argument("Event intervention watches a different event");
            }
            validate_rule(rule);
        }
    }
    for (const auto& rule : definition_.interceptors) {
        validate_rule(rule);
    }
}

const Flow& EventSystem::flow(const Id& event) const {
    const auto& id = definition_.events.at(event).flow;
    if (!id) {
        throw std::invalid_argument("Event has no flow");
    }
    return definition_.flows.at(*id);
}

const Node& EventSystem::node(const Id& event) const {
    return flow(event).nodes.at(progress_.at(event).node);
}

int32_t EventSystem::visits(const Id& event) const {
    return progress_.at(event).visits;
}

void EventSystem::set_facts(Facts facts) {
    if (!std::ranges::all_of(definition_.external_conditions, [&](const Id& id) { return facts.contains(id); })) {
        throw std::invalid_argument("Missing external condition results");
    }
    facts_ = std::move(facts);
}

bool EventSystem::can_enter(const Id& event, EntryMode mode) const {
    const auto definition = definition_.events.find(event);
    if (definition == definition_.events.end() || !definition->second.flow) {
        return false;
    }
    const auto& progress = progress_.at(event);
    return mode == EntryMode::start || (progress.visits > 0 && !progress.finished);
}

bool EventSystem::matches(const Condition& condition, const Id* choice) const {
    return (!condition.choice || (choice && *condition.choice == *choice)) &&
        std::ranges::all_of(condition.parameters, [&](const auto& check) {
            return matches_value(parameters_.at(check.set).at(check.parameter), check.comparison, check.value);
        }) &&
        std::ranges::all_of(condition.external, [&](const auto& entry) {
            const auto fact = facts_.find(entry.first);
            return fact != facts_.end() && fact->second == entry.second;
        });
}

void EventSystem::reset() {
#ifdef ADVENTURE_DEBUG_JUMP
    saved_debug_.reset();
#endif
    parameters_ = definition_.parameters;
    for (auto& [event, progress] : progress_) {
        progress = {};
    }
    current_.reset();
    return_points_.clear();
    last_visible_.reset();
    const auto [generation, revision] = info_.stamp;
    info_ = {};
    info_.stamp = {generation + 1, revision + 1};
}

bool EventSystem::start(const Id& event, EntryMode mode) {
    if (!can_enter(event, mode) ||
        !std::ranges::all_of(definition_.external_conditions, [&](const Id& id) { return facts_.contains(id); })) {
        return false;
    }
    return_points_.clear();
    begin(event, mode);
    settle();
    refresh();
    return true;
}

void EventSystem::begin(const Id& event, EntryMode mode) {
    current_ = event;
    info_.status = Status::active;
    if (mode == EntryMode::start) {
        auto& progress = progress_.at(event);
        const auto visits = progress.visits;
        progress = {.node = flow(event).start, .visits = visits + 1};
    }
}

std::optional<Id> EventSystem::next(const Id& event) const {
    const auto& next_node = node(event).next;
    if (const auto* fixed = std::get_if<Id>(&next_node)) {
        return *fixed;
    }
    const auto& selected = progress_.at(event).selected;
    if (selected && std::holds_alternative<SelectedDestination>(next_node)) {
        return selected->destination;
    }
    return std::nullopt;
}

void EventSystem::transition(const Id& event, Id to) {
    auto& progress = progress_.at(event);
    const auto* choice = node(event).kind == NodeKind::choice && progress.selected ? &progress.selected->id : nullptr;
    const auto rules = rules_.find({event, progress.node, to});
    std::vector<Choice> added;
    const EventTransfer* transfer = nullptr;
    if (rules != rules_.end()) {
#ifdef ADVENTURE_DEBUG_JUMP
        const auto record = [&](JumpIssue issue) {
            if (jump_issues_ && jump_issues_->size() < 64 &&
                std::ranges::find(*jump_issues_, issue) == jump_issues_->end()) {
                jump_issues_->push_back(std::move(issue));
            }
        };
        const auto diagnose = [&](const Condition& condition, bool after, const Id& target) {
            if (!jump_issues_ || (condition.choice && (!choice || *choice != *condition.choice))) {
                return;
            }
            for (const auto& check : condition.parameters) {
                const auto& actual = parameters_.at(check.set).at(check.parameter);
                if (!matches_value(actual, check.comparison, check.value)) {
                    record({.kind = JumpIssueKind::parameter, .edge = rules->first, .after_update = after,
                        .set = check.set, .name = check.parameter, .comparison = check.comparison,
                        .expected = check.value, .actual = actual, .target = target});
                }
            }
            for (const auto& [name, expected] : condition.external) {
                const auto fact = facts_.find(name);
                if (fact == facts_.end() || fact->second != expected) {
                    record({.kind = JumpIssueKind::external, .edge = rules->first, .after_update = after,
                        .name = name, .expected = Value{expected},
                        .actual = fact == facts_.end() ? std::nullopt : std::optional<Value>{fact->second},
                        .target = target});
                }
            }
        };
        std::vector<const EventTransfer*> eligible;
#endif
        std::vector<const ParameterOperation*> operations;
        // Test every operation against the same starting state.
        for (const auto* rule : rules->second) {
            if (rule->parameter) {
#ifdef ADVENTURE_DEBUG_JUMP
                diagnose(rule->parameter->condition, false, {});
#endif
                if (matches(rule->parameter->condition, choice)) {
                    operations.push_back(&rule->parameter->action);
                }
            }
        }
        for (const auto* operation : operations) {
            apply(parameters_, *operation);
        }
        for (const auto* rule : rules->second) {
            if (!rule->effect) {
                continue;
            }
#ifdef ADVENTURE_DEBUG_JUMP
            const auto* destination = std::get_if<EventTransfer>(&rule->effect->action);
            diagnose(rule->effect->condition, true, destination ? destination->event : Id{});
#endif
            if (!matches(rule->effect->condition, choice)) {
                continue;
            }
            if (const auto* addition = std::get_if<Choice>(&rule->effect->action)) {
                added.push_back(*addition);
            } else {
                const auto& candidate = std::get<EventTransfer>(rule->effect->action);
                if (!can_enter(candidate.event, candidate.mode)) {
#ifdef ADVENTURE_DEBUG_JUMP
                    if (jump_issues_) {
                        record({.kind = JumpIssueKind::resume, .edge = rules->first,
                            .actual = Value{progress_.at(candidate.event).finished ? Id{L"finished"} : Id{L"not-started"}},
                            .target = candidate.event});
                    }
#endif
                    continue;
                }
#ifdef ADVENTURE_DEBUG_JUMP
                if (jump_issues_) {
                    eligible.push_back(&candidate);
                }
#endif
                if (!transfer || candidate.priority > transfer->priority ||
                    (candidate.priority == transfer->priority && candidate.event < transfer->event)) {
                    transfer = &candidate;
                }
            }
        }
#ifdef ADVENTURE_DEBUG_JUMP
        for (const auto* candidate : eligible) {
            if (candidate != transfer) {
                record({.kind = JumpIssueKind::priority, .edge = rules->first, .expected = Value{candidate->event},
                    .actual = Value{transfer->event}, .priority = candidate->priority,
                    .selected_priority = transfer->priority, .target = candidate->event});
            }
        }
#endif
    }
    if (flow(event).nodes.at(to).kind == NodeKind::choice) {
        progress.choices = std::move(added);
    }
    if (!transfer) {
        progress.node = std::move(to);
        return;
    }
    if (transfer->call && transfer->event != event) {
        return_points_.push_back({event, std::move(to), progress.selected, progress.choices});
    } else {
        progress.finished = true;
    }
    begin(transfer->event, transfer->mode);
}

bool EventSystem::finish(const Id& event) {
    progress_.at(event).finished = true;
    if (definition_.events.at(event).ending == EndBehavior::return_to_caller && !return_points_.empty()) {
        auto point = std::move(return_points_.back());
        return_points_.pop_back();
        auto& progress = progress_.at(point.event);
        progress.node = std::move(point.node);
        progress.selected = std::move(point.selected);
        progress.choices = std::move(point.choices);
        begin(point.event, EntryMode::resume);
        return true;
    }
    return_points_.clear();
    info_.status = Status::completed;
    return false;
}

void EventSystem::settle() {
    std::set<std::pair<Id, Id>> visited;
    while (current_) {
        const auto& event = *current_;
        const auto& position = progress_.at(event).node;
        if (!visited.emplace(event, position).second) {
            return_points_.clear();
            info_.status = Status::completed;
            return;
        }
        switch (node(event).kind) {
        case NodeKind::automatic: {
            auto destination = next(event);
            if (!destination) {
                info_.status = Status::completed;
                return;
            }
            transition(event, std::move(*destination));
            break;
        }
        case NodeKind::end:
            if (!finish(event)) {
                return;
            }
            break;
        case NodeKind::advance:
        case NodeKind::choice:
            last_visible_ = std::pair{event, position};
            return;
        }
    }
}

void EventSystem::refresh() {
    info_.presentation_request.reset();
    info_.choices.clear();
    if (info_.status == Status::completed) {
        if (last_visible_) {
            const auto& [event, node_id] = *last_visible_;
            if (info_.event != event || info_.node != node_id) {
                ++info_.stamp.revision;
            }
            info_.event = event;
            info_.flow = *definition_.events.at(event).flow;
            info_.node = node_id;
            info_.kind = flow(event).nodes.at(node_id).kind;
        }
        return;
    }
    const auto& event = *current_;
    const auto& progress = progress_.at(event);
    const auto& current_node = node(event);
    info_.event = event;
    info_.flow = *definition_.events.at(event).flow;
    info_.node = progress.node;
    info_.kind = current_node.kind;
    info_.presentation_request = ++info_.stamp.revision;
    if (current_node.kind == NodeKind::choice) {
        info_.choices = current_node.choices;
        info_.choices.insert(info_.choices.end(), progress.choices.begin(), progress.choices.end());
    }
}

bool EventSystem::accepts(Stamp stamp, NodeKind kind) const {
    return current_ && info_.status == Status::active && !info_.presentation_request &&
        stamp == info_.stamp && info_.kind == kind;
}

bool EventSystem::advance(Stamp stamp) {
    if (!accepts(stamp, NodeKind::advance)) {
        return false;
    }
    auto destination = next(*current_);
    if (!destination) {
        return false;
    }
    transition(*current_, std::move(*destination));
    settle();
    refresh();
    return true;
}

bool EventSystem::choose(Stamp stamp, const Id& choice) {
    if (!accepts(stamp, NodeKind::choice)) {
        return false;
    }
    const auto selected = std::ranges::find(info_.choices, choice, &Choice::id);
    if (selected == info_.choices.end()) {
        return false;
    }
    auto& progress = progress_.at(*current_);
    const auto previous = progress.selected;
    progress.selected = *selected;
    auto destination = next(*current_);
    if (!destination) {
        progress.selected = previous;
        return false;
    }
    transition(*current_, std::move(*destination));
    settle();
    refresh();
    return true;
}

bool EventSystem::presentation_ready(uint64_t request) {
    if (info_.presentation_request != request) {
        return false;
    }
    info_.presentation_request.reset();
    if (current_ && info_.status == Status::active && node(*current_).kind == NodeKind::advance &&
        definition_.events.at(*current_).ending == EndBehavior::end && next(*current_) == post_event) {
        transition(*current_, post_event);
        settle();
        refresh();
    }
    return true;
}

}
