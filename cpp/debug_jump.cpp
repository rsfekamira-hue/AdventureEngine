#include "event_system.h"

#include <algorithm>
#include <deque>
#include <tuple>
#include <utility>

namespace adventure {

EventSystem::DebugState EventSystem::debug_state() const {
    return {parameters_, progress_, current_, return_points_, last_visible_, info_};
}

void EventSystem::restore_debug_state(DebugState state) {
    parameters_ = std::move(state.parameters);
    progress_ = std::move(state.progress);
    current_ = std::move(state.current);
    return_points_ = std::move(state.return_points);
    last_visible_ = std::move(state.last_visible);
    info_ = std::move(state.info);
}

std::optional<JumpReport> EventSystem::debug_jump(const Id& event, const std::optional<Id>& node, size_t limit) {
    if (limit == 0 || !can_enter(event, EntryMode::start)) {
        return std::nullopt;
    }
    const auto& target = flow(event);
    const auto visible = [](const Node& entry) {
        return entry.kind == NodeKind::advance || entry.kind == NodeKind::choice;
    };
    const auto first_visible = std::ranges::find_if(target.nodes, [&](const auto& entry) {
        return visible(entry.second);
    });
    const auto selected = node ? target.nodes.find(*node) : first_visible;
    if (selected == target.nodes.end() || !visible(selected->second)) {
        return std::nullopt;
    }

    using Location = std::pair<Id, Id>;
    std::map<Location, std::vector<Location>> incoming;
    const auto edge = [&](const Id& id, const Id& from, const Id& to) {
        incoming[{id, to}].emplace_back(id, from);
    };
    for (const auto& [id, definition] : definition_.events) {
        if (!definition.flow) {
            continue;
        }
        for (const auto& [key, entry] : flow(id).nodes) {
            const auto* fixed = std::get_if<Id>(&entry.next);
            if (fixed) {
                edge(id, key, *fixed);
            }
            for (const auto& choice : entry.choices) {
                edge(id, key, choice.destination);
                if (fixed) {
                    edge(id, *fixed, choice.destination);
                }
            }
        }
    }
    for (const auto& [target_edge, rules] : rules_) {
        for (const auto* rule : rules) {
            if (!rule->effect) {
                continue;
            }
            if (const auto* transfer = std::get_if<EventTransfer>(&rule->effect->action)) {
                incoming[{transfer->event, flow(transfer->event).start}].emplace_back(target_edge.event, target_edge.from);
            } else {
                edge(target_edge.event, target_edge.to, std::get<Choice>(rule->effect->action).destination);
            }
        }
    }
    std::map<Location, size_t> distance;
    std::deque<Location> backwards;
    for (const auto& [key, entry] : target.nodes) {
        if ((node && key == *node) || (!node && visible(entry))) {
            backwards.emplace_back(event, key);
            distance[backwards.back()] = 0;
        }
    }
    while (!backwards.empty()) {
        const auto position = std::move(backwards.front());
        backwards.pop_front();
        const auto edges = incoming.find(position);
        if (edges == incoming.end()) {
            continue;
        }
        for (const auto& from : edges->second) {
            if (distance.try_emplace(from, distance.at(position) + 1).second) {
                backwards.push_back(from);
            }
        }
    }

    struct Candidate {
        DebugState state;
        std::vector<Id> route;
        std::vector<JumpIssue> issues;
    };
    using Key = std::tuple<Status, std::optional<Id>, Parameters, std::map<Id, Progress>, std::vector<ReturnPoint>>;
    std::set<Key> seen;
    std::multimap<size_t, Candidate> queue;
    const auto enqueue = [&](const std::vector<Id>& route, const std::vector<JumpIssue>& issues) {
        if (queue.size() >= limit) {
            return;
        }
        auto normalized = progress_;
        for (auto& [id, value] : normalized) {
            value.visits = value.visits > 0 ? 1 : 0;
        }
        if (seen.emplace(info_.status, current_, parameters_, std::move(normalized), return_points_).second) {
            const auto remaining = distance.find({info_.event, info_.node});
            const auto rank = remaining == distance.end() ? 10000 : remaining->second;
            queue.emplace(route.size() + rank, Candidate{debug_state(), route, issues});
        }
    };
    auto original = debug_state();
    JumpReport report;
    std::tuple<bool, size_t, size_t> best{true, 10001, 0};
    const auto remember = [&](const std::vector<Id>& route, const std::vector<JumpIssue>& issues) {
        for (const auto& issue : issues) {
            const auto remaining = distance.find({issue.edge.event, issue.edge.from});
            const auto rank = std::tuple{issue.target != event,
                remaining == distance.end() ? size_t{10000} : remaining->second, route.size()};
            if (rank < best) {
                best = rank;
                report.route = route;
                report.issues = issues;
                report.stopped = issue.edge.event + L"/" + issue.edge.from;
            }
        }
    };
    try {
        enqueue({}, {});
        while (!queue.empty() && report.explored < limit) {
            auto entry = queue.extract(queue.begin());
            auto [state, route, issues] = std::move(entry.mapped());
            restore_debug_state(std::move(state));
            ++report.explored;
            if (info_.status == Status::active && info_.event == event && (!node || info_.node == *node)) {
                report.route = std::move(route);
                report.issues = std::move(issues);
                report.stopped = info_.event + L"/" + info_.node;
                info_.stamp = original.info.stamp;
                refresh();
                saved_debug_ = std::move(original);
                return report;
            }
            jump_issues_ = &issues;
            for (size_t pass = 0; info_.presentation_request && pass < limit; ++pass) {
                presentation_ready(*info_.presentation_request);
            }
            jump_issues_ = nullptr;
            const auto ready = debug_state();
            remember(route, issues);
            if (info_.kind == NodeKind::choice) {
                if (ready.info.choices.empty() && issues.size() < 64) {
                    issues.push_back({.kind = JumpIssueKind::no_choices, .edge = {info_.event, info_.node, {}}});
                    remember(route, issues);
                }
                for (const auto& choice : ready.info.choices) {
                    restore_debug_state(ready);
                    auto branch = issues;
                    jump_issues_ = &branch;
                    if (choose(ready.info.stamp, choice.id)) {
                        route.push_back(L"choose-id " + choice.id);
                        remember(route, branch);
                        enqueue(route, branch);
                        route.pop_back();
                    }
                    jump_issues_ = nullptr;
                }
            } else {
                jump_issues_ = &issues;
                if (advance(ready.info.stamp)) {
                    route.push_back(L"advance");
                    remember(route, issues);
                    enqueue(route, issues);
                }
                jump_issues_ = nullptr;
            }
        }
        report.forced = true;
        report.limited = !queue.empty();
        std::stable_partition(report.issues.begin(), report.issues.end(), [&](const JumpIssue& issue) {
            return issue.target == event;
        });
        if (report.stopped.empty()) {
            report.stopped = info_.event + L"/" + info_.node;
        }
        restore_debug_state(original);
        if (node || !start(event) || info_.event != event || info_.status != Status::active) {
            restore_debug_state(original);
            return_points_.clear();
            begin(event, EntryMode::start);
            progress_.at(event).node = selected->first;
            refresh();
        }
        saved_debug_ = std::move(original);
        return report;
    } catch (...) {
        jump_issues_ = nullptr;
        restore_debug_state(std::move(original));
        throw;
    }
}

bool EventSystem::debug_restore() {
    if (!saved_debug_) {
        return false;
    }
    const auto latest = info_.stamp;
    restore_debug_state(std::move(*saved_debug_));
    saved_debug_.reset();
    info_.stamp = {latest.generation, latest.revision + 1};
    if (info_.status == Status::active) {
        refresh();
    }
    return true;
}

bool EventSystem::debug_parameter(const Id& set, const Id& name, Value value) {
    const auto group = parameters_.find(set);
    if (group == parameters_.end()) {
        return false;
    }
    const auto parameter = group->second.find(name);
    if (parameter == group->second.end() || parameter->second.index() != value.index()) {
        return false;
    }
    parameter->second = std::move(value);
    return true;
}

std::optional<Id> EventSystem::debug_next() const {
    return current_ && info_.status == Status::active ? next(*current_) : std::nullopt;
}

std::vector<DebugWatch> EventSystem::debug_watches(const Id& event, bool current_only) const {
    std::vector<DebugWatch> result;
    const auto current = current_ && info_.status == Status::active;
    for (const auto& [edge, rules] : rules_) {
        const bool at_node = current && edge.event == *current_ && edge.from == info_.node;
        if (edge.event != event || (current_only && !at_node)) {
            continue;
        }
        const auto append = [&](const Condition& condition, Id action, Id target) {
            // Evaluate a choice-gated rule for its declared choice before the player selects it.
            const auto* choice = condition.choice ? &*condition.choice : nullptr;
            result.push_back({edge, std::move(action), std::move(target), condition.choice, at_node,
                matches(condition, choice)});
        };
        for (const auto* rule : rules) {
            if (rule->parameter) {
                const auto& operation = rule->parameter->action;
                append(rule->parameter->condition, operation.update == Update::set ? L"set" : L"add",
                    operation.set + L"." + operation.parameter);
            }
            if (rule->effect) {
                if (const auto* transfer = std::get_if<EventTransfer>(&rule->effect->action)) {
                    append(rule->effect->condition, transfer->call ? L"call" : L"switch", transfer->event);
                } else {
                    append(rule->effect->condition, L"add-choice", std::get<Choice>(rule->effect->action).id);
                }
            }
        }
    }
    return result;
}

}
