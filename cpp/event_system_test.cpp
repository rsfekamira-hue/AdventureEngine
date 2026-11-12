#include "event_system.h"

#include <iostream>
#include <stdexcept>

using namespace adventure;

namespace {

void check(bool condition, const char* message) {
    if (!condition) {
        throw std::runtime_error(message);
    }
}

Definition example() {
    Definition result;
    result.parameters[L"shared"] = {{L"count", int32_t{0}}, {L"seen", false}};
    result.flows[L"main"] = {L"entry", {
        {L"entry", {NodeKind::automatic, Id{L"menu"}, {}}},
        {L"menu", {NodeKind::choice, Id{L"post"}, {}}},
        {L"post", {NodeKind::automatic, SelectedDestination{}, {}}},
        {L"line", {NodeKind::advance, Id{L"@event:end"}, {}}},
        {L"@event:end", {NodeKind::end, {}, {}}},
    }};
    result.flows[L"side"] = {L"line", {
        {L"line", {NodeKind::advance, Id{L"end"}, {}}},
        {L"end", {NodeKind::end, {}, {}}},
    }};
    result.events[L"main"] = {Id{L"main"}, L"shared", EndBehavior::end, {}};
    result.events[L"side"] = {Id{L"side"}, L"shared", EndBehavior::return_to_caller, {}};
    result.events[L"alternative"] = {Id{L"side"}, L"shared", EndBehavior::return_to_caller, {}};
    auto& rules = result.events.at(L"main").interventions;
    const Edge post{L"main", L"menu", L"post"};
    const Condition before{Id{L"visit"}, {{L"shared", L"count", Comparison::eq, int32_t{0}}}, {}};
    rules.push_back({post, Conditional<ParameterOperation>{before,
        {L"shared", L"count", Update::add, int32_t{1}}}, {}});
    rules.push_back({post, Conditional<ParameterOperation>{before,
        {L"shared", L"seen", Update::set, true}}, {}});
    const Condition after{{}, {{L"shared", L"seen", Comparison::eq, true}}, {}};
    rules.push_back({post, {}, Conditional<Effect>{after,
        EventTransfer{L"side", EntryMode::start, 10, true}}});
    result.interceptors.push_back({post, {}, Conditional<Effect>{after,
        EventTransfer{L"alternative", EntryMode::start, 5, false}}});
    result.interceptors.push_back({{L"main", L"entry", L"menu"}, {},
        Conditional<Effect>{{}, Choice{L"visit", L"line"}}});
    return result;
}

void ready(EventSystem& system) {
    const auto request = system.info().presentation_request;
    check(request.has_value() && system.presentation_ready(*request), "presentation request was not released");
}

}

int main() {
    try {
        EventSystem system(example());
        check(system.start(L"main"), "start failed");
        const auto original_stamp = system.info().stamp;
        check(system.info().choices.size() == 1, "interceptor choice missing");
        check(!system.choose(original_stamp, L"visit"), "input accepted during presentation");
        ready(system);
        check(system.choose(original_stamp, L"visit"), "choice failed");
        check(system.info().event == L"side" && system.visits(L"alternative") == 0, "event priority changed");
        check(std::get<int32_t>(system.parameters().at(L"shared").at(L"count")) == 1, "add failed");
        check(std::get<bool>(system.parameters().at(L"shared").at(L"seen")), "operations used different starting values");
        ready(system);
        check(!system.advance(original_stamp), "stale input accepted");
        check(system.advance(system.info().stamp), "side event did not advance");
        check(system.info().event == L"main" && system.info().node == L"line", "return point lost the choice destination");
        ready(system);
        check(system.info().status == Status::completed && system.info().node == L"line", "ending was not retained");
        system.reset();
        check(system.info().status == Status::idle && system.visits(L"side") == 0, "reset did not clear progress");
        check(!std::get<bool>(system.parameters().at(L"shared").at(L"seen")), "reset did not restore initial parameters");
#ifdef ADVENTURE_DEBUG_JUMP
        check(system.start(L"main"), "debug test could not start");
        const auto before_jump = system.info().stamp;
        const auto route = system.debug_jump(L"side", Id{L"line"});
        check(route && !route->forced && route->route == std::vector<Id>{L"choose-id visit"},
            "debug jump did not follow the real choice");
        check(system.info().event == L"side" && system.visits(L"side") == 1, "debug route was not adopted");
        check(std::get<int32_t>(system.parameters().at(L"shared").at(L"count")) == 1,
            "debug route did not apply the parameter update");
        check(system.debug_restore() && system.info().node == L"menu" && system.info().stamp != before_jump &&
            system.visits(L"side") == 0, "debug restore lost the original state");
        check(!system.debug_jump(L"main", Id{L"entry"}) && system.info().node == L"menu",
            "generated node was accepted as a debug target");
        check(!system.debug_parameter(L"shared", L"count", false) &&
            system.debug_parameter(L"shared", L"count", int32_t{2}), "debug parameter type was not enforced");
        const auto forced = system.debug_jump(L"alternative", Id{L"line"}, 1);
        check(forced && forced->forced && forced->limited && system.info().event == L"alternative",
            "bounded search did not distinguish a forced jump");
        check(std::get<int32_t>(system.parameters().at(L"shared").at(L"count")) == 2,
            "forced jump applied updates from unselected paths");
#endif
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
