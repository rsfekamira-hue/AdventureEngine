#pragma once

#include <compare>
#include <cstdint>
#include <map>
#include <optional>
#include <set>
#include <string>
#include <variant>
#include <vector>

namespace adventure {

using Id = std::wstring;
using Value = std::variant<bool, int32_t, std::wstring>;
using Parameters = std::map<Id, std::map<Id, Value>>;
using Facts = std::map<Id, bool>;

enum class NodeKind { automatic, advance, choice, end };
enum class EntryMode { start, resume };
enum class EndBehavior { return_to_caller, end };
enum class Status { idle, active, completed };
enum class Comparison { eq, ne, gt, gte, lt, lte };
enum class Update { set, add };

struct SelectedDestination {};
using NextNode = std::variant<std::monostate, Id, SelectedDestination>;

struct Choice {
    Id id;
    Id destination;
    auto operator<=>(const Choice&) const = default;
};

struct Node {
    NodeKind kind;
    NextNode next;
    std::vector<Choice> choices;
};

struct Flow {
    Id start;
    std::map<Id, Node> nodes;
};

struct ParameterCondition {
    Id set;
    Id parameter;
    Comparison comparison;
    Value value;
};

struct Condition {
    std::optional<Id> choice;
    std::vector<ParameterCondition> parameters;
    Facts external;
};

struct ParameterOperation {
    Id set;
    Id parameter;
    Update update;
    Value value;
};

struct EventTransfer {
    Id event;
    EntryMode mode = EntryMode::start;
    int32_t priority = 0;
    bool call = false;
};
using Effect = std::variant<Choice, EventTransfer>;

template<class Action>
struct Conditional {
    Condition condition;
    Action action;
};

struct Edge {
    Id event;
    Id from;
    Id to;
    auto operator<=>(const Edge&) const = default;
};

struct Intervention {
    Edge edge;
    std::optional<Conditional<ParameterOperation>> parameter;
    std::optional<Conditional<Effect>> effect;
};

struct Event {
    std::optional<Id> flow;
    Id parameter_set;
    EndBehavior ending = EndBehavior::return_to_caller;
    std::vector<Intervention> interventions;
};

struct Definition {
    std::map<Id, Flow> flows;
    std::map<Id, Event> events;
    Parameters parameters;
    std::vector<Intervention> interceptors;
    std::set<Id> external_conditions;
};

struct Stamp {
    uint64_t generation = 0;
    uint64_t revision = 0;
    bool operator==(const Stamp&) const = default;
};

struct EventInfo {
    Stamp stamp;
    Status status = Status::idle;
    Id event;
    Id flow;
    Id node;
    NodeKind kind = NodeKind::end;
    std::vector<Choice> choices;
    std::optional<uint64_t> presentation_request;
};

#ifdef ADVENTURE_DEBUG_JUMP
enum class JumpIssueKind { parameter, external, resume, priority, no_choices };

struct JumpIssue {
    JumpIssueKind kind;
    Edge edge;
    bool after_update = false;
    Id set;
    Id name;
    Comparison comparison = Comparison::eq;
    std::optional<Value> expected;
    std::optional<Value> actual;
    std::optional<int32_t> priority;
    std::optional<int32_t> selected_priority;
    Id target;
    bool operator==(const JumpIssue&) const = default;
};

struct JumpReport {
    bool forced = false;
    bool limited = false;
    size_t explored = 0;
    std::vector<Id> route;
    std::vector<JumpIssue> issues;
    Id stopped;
};

struct DebugWatch {
    Edge edge;
    Id action;
    Id target;
    std::optional<Id> choice;
    bool matches_node;
    bool conditions_met;
};
#endif

class EventSystem {
public:
    explicit EventSystem(Definition definition);
    EventSystem(const EventSystem&) = delete;
    EventSystem& operator=(const EventSystem&) = delete;

    const EventInfo& info() const { return info_; }
    const Parameters& parameters() const { return parameters_; }
    int32_t visits(const Id& event) const;
    void set_facts(Facts facts);

    void reset();
    bool start(const Id& event, EntryMode mode = EntryMode::start);
    bool advance(Stamp stamp);
    bool choose(Stamp stamp, const Id& choice);
    bool presentation_ready(uint64_t request);
#ifdef ADVENTURE_DEBUG_JUMP
    std::optional<JumpReport> debug_jump(const Id& event, const std::optional<Id>& node, size_t limit = 1024);
    bool debug_restore();
    bool debug_parameter(const Id& set, const Id& name, Value value);
    std::vector<DebugWatch> debug_watches(const Id& event, bool current_only) const;
    std::optional<Id> debug_next() const;
#endif

private:
    struct Progress {
        Id node;
        std::optional<Choice> selected;
        std::vector<Choice> choices;
        int32_t visits = 0;
        bool finished = false;
        auto operator<=>(const Progress&) const = default;
    };
    struct ReturnPoint {
        Id event;
        Id node;
        std::optional<Choice> selected;
        std::vector<Choice> choices;
        auto operator<=>(const ReturnPoint&) const = default;
    };

    Definition definition_;
    Parameters parameters_;
    Facts facts_;
    std::map<Id, Progress> progress_;
    std::map<Edge, std::vector<const Intervention*>> rules_;
    std::optional<Id> current_;
    std::vector<ReturnPoint> return_points_;
    std::optional<std::pair<Id, Id>> last_visible_;
    EventInfo info_;

#ifdef ADVENTURE_DEBUG_JUMP
    struct DebugState {
        Parameters parameters;
        std::map<Id, Progress> progress;
        std::optional<Id> current;
        std::vector<ReturnPoint> return_points;
        std::optional<std::pair<Id, Id>> last_visible;
        EventInfo info;
    };
    std::optional<DebugState> saved_debug_;
    std::vector<JumpIssue>* jump_issues_ = nullptr;
    DebugState debug_state() const;
    void restore_debug_state(DebugState state);
#endif

    void validate() const;
    bool can_enter(const Id& event, EntryMode mode) const;
    bool accepts(Stamp stamp, NodeKind kind) const;
    bool matches(const Condition& condition, const Id* choice) const;
    const Flow& flow(const Id& event) const;
    const Node& node(const Id& event) const;
    std::optional<Id> next(const Id& event) const;
    void begin(const Id& event, EntryMode mode);
    void transition(const Id& event, Id to);
    bool finish(const Id& event);
    void settle();
    void refresh();
};

}
