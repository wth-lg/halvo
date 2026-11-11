--------------------------- MODULE ScheduleTrace ---------------------------
EXTENDS Schedule, TraceText

CONSTANTS Seed, Count, Depth

VARIABLE w

RefusePct == 12
PortSeq == <<"ok", "no_ack", "no_run">>
FrameSeq == <<"completed", "nothing_to_do", "declined", "handed_back", "class_mismatch",
              "refused", "limit", "effect_unknown", "failed", "none">>
KindSeq == <<"start", "hello", "wait", "resume", "stop", "end", "tick", "restart", "delete", "off">>
RefusalSeq == <<"hold", "budget", "state", "queue_full", "no_service">>

Weight(k) ==
    CASE k = "start" -> 30
      [] k = "hello" -> 14
      [] k = "wait" -> 8
      [] k = "resume" -> 8
      [] k = "stop" -> 5
      [] k = "end" -> 12
      [] k = "tick" -> 16
      [] k = "restart" -> 2
      [] k = "delete" -> 3
      [] k = "off" -> 2

RECURSIVE WSum(_)
WSum(ks) == IF Len(ks) = 0 THEN 0 ELSE Weight(ks[1]) + WSum(Tail(ks))

RECURSIVE WPick(_, _)
WPick(ks, t) ==
    IF Len(ks) = 1 \/ t < Weight(ks[1]) THEN ks[1]
    ELSE WPick(Tail(ks), t - Weight(ks[1]))

S(a, p) == [m |-> "start", agent |-> a, port |-> p]
T(d, g, p) == [m |-> "tick", day |-> d, grace |-> g, port |-> p]
E(id, f, p) == [m |-> "end", run |-> id, frame |-> f, port |-> p]
X(id, c) == [m |-> "stop", run |-> id, cause |-> c]
B(d) == [m |-> "restart", day |-> d]
D(a) == [m |-> "delete", agent |-> a]
O(a) == [m |-> "off", agent |-> a]
H(id) == [m |-> "hello", run |-> id]

Directed ==
    <<[case |-> "two_models", moves |-> <<S(1, "ok"), H(1), S(2, "ok")>>],
      [case |-> "two_runs_agent", moves |-> <<S(1, "ok"), S(2, "ok"), S(2, "ok"), S(1, "ok")>>],
      [case |-> "hold_start",
       moves |-> <<T(1, TRUE, "no_run"), S(2, "no_run"), T(2, TRUE, "ok"), T(3, TRUE, "ok"), S(2, "ok"),
                   O(3), S(3, "ok"), S(4, "ok"), E(4, "effect_unknown", "ok"), S(4, "ok")>>],
      [case |-> "unrecorded_start", moves |-> <<S(1, "no_ack")>>],
      [case |-> "second_slot_run", moves |-> <<T(1, TRUE, "ok"), B(1), T(1, TRUE, "ok")>>],
      [case |-> "auto_needs_person", moves |-> <<T(1, TRUE, "ok")>>],
      [case |-> "over_budget",
       moves |-> <<S(4, "ok"), E(1, "completed", "ok"), S(4, "ok"), E(2, "completed", "ok"), S(4, "ok")>>],
      [case |-> "queue_full", moves |-> <<S(1, "ok"), S(3, "ok"), S(2, "ok"), S(4, "ok"), S(5, "ok")>>],
      [case |-> "outlives_app", moves |-> <<S(1, "ok"), S(3, "ok"), X(1, "cancelled"), B(1)>>],
      [case |-> "stop_tree",
       moves |-> <<S(1, "ok"), H(1), X(1, "cancelled"), T(1, FALSE, "ok"), T(1, TRUE, "ok")>>],
      [case |-> "deleted_workflow", moves |-> <<S(4, "ok"), D(4), T(1, TRUE, "ok"), T(1, TRUE, "ok")>>]>>

StartCands(s) == Cat([a \in 1..NAgents |-> [i \in 1..3 |-> S(a, PortSeq[i])]])

LiveCands(s, id) ==
    <<H(id), [m |-> "wait", run |-> id], [m |-> "resume", run |-> id], X(id, "cancelled")>>
    \o (IF (CHOOSE r \in s.runs : r.id = id).phase = "waiting" THEN <<X(id, "lapsed")>> ELSE <<>>)
    \o [f \in 1..Len(FrameSeq) |-> E(id, FrameSeq[f], "ok")]

RunCands(s) ==
    Cat([id \in 1..s.nrun |->
            IF \E r \in s.runs : r.id = id
            THEN LiveCands(s, id)
            ELSE <<H(id), E(id, "none", "ok")>>])

TickCands(s) ==
    <<T(s.day, TRUE, "ok"), T(s.day, TRUE, "ok")>>
    \o (IF s.day < MaxDay THEN <<T(s.day + 1, TRUE, "ok")>> ELSE <<>>)

OtherCands(s) ==
    (IF s.life <= MaxRestarts THEN <<B(s.day)>> ELSE <<>>)
    \o Cat([a \in 1..NAgents |->
              IF s.dels < MaxDeletes /\ a \in Deletable \ s.gone THEN <<D(a)>> ELSE <<>>])
    \o Cat([a \in 1..NAgents |-> IF a \in OffAgents \ s.off THEN <<O(a)>> ELSE <<>>])

Cands(s) == StartCands(s) \o RunCands(s) \o TickCands(s) \o OtherCands(s)

RECURSIVE AnswersOf(_, _, _)
AnswersOf(s, cs, i) ==
    IF i > Len(cs) THEN <<>> ELSE <<Step(s, cs[i]).r>> \o AnswersOf(s, cs, i + 1)

Choose(s, x0) ==
    LET cs == Cands(s)
        as == AnswersOf(s, cs, 1)
        IsGood(i) == as[i] \in {"ok", "queued"}
        IsBad(i) == as[i] \notin {"ok", "queued"}
        idx == [i \in 1..Len(cs) |-> i]
        good == SelectSeq(idx, IsGood)
        bad == SelectSeq(idx, IsBad)
        x1 == Lcg(x0)
        x2 == Lcg(x1)
        x3 == Lcg(x2)
        x4 == Lcg(x3)
        x5 == Lcg(x4)
        x6 == Lcg(x5)
        refuse == Len(bad) > 0 /\ (Len(good) = 0 \/ Draw(x1, 100) < RefusePct)
        HasAnswer(a) == \E j \in 1..Len(bad) : as[bad[j]] = a
        answers == SelectSeq(RefusalSeq, HasAnswer)
        answer == answers[1 + Draw(x2, Len(answers))]
        WithAnswer(i) == as[i] = answer
        HasKind(k) == \E j \in 1..Len(good) : cs[good[j]].m = k
        kinds == SelectSeq(KindSeq, HasKind)
        kind == WPick(kinds, Draw(x2, WSum(kinds)))
        OfKind(i) == cs[i].m = kind
        pool == IF refuse THEN SelectSeq(bad, WithAnswer) ELSE SelectSeq(good, OfKind)
        base == cs[pool[1 + Draw(x3, Len(pool))]]
        port == LET d == Draw(x4, 8)
                IN IF d < 6 THEN "ok" ELSE IF d = 6 THEN "no_ack" ELSE "no_run"
        mv == IF base.m = "tick"
              THEN [base EXCEPT !.port = port,
                                !.grace = (\A r \in s.runs : r.phase # "stopping") \/ Draw(x6, 3) # 0]
              ELSE IF base.m = "end" /\ Len(s.queue) > 0 THEN [base EXCEPT !.port = port]
              ELSE IF base.m = "restart" /\ s.day < MaxDay /\ Draw(x5, 4) = 0 THEN B(s.day + 1)
              ELSE base
    IN [mv |-> mv, x |-> x6]

RunName(n) == IF n = 0 THEN "null" ELSE Id("r", n)
AgentList(xs) == Arr([i \in 1..Len(xs) |-> Id("a", xs[i])])
RunList(set) == LET ids == Sorted(set) IN Arr([i \in 1..Len(ids) |-> Id("r", ids[i])])
YesNo(b) == IF b THEN Str("yes") ELSE Str("no")
StartedJson(x) ==
    Obj(<<KV("agent", Id("a", x.agent)), KV("r", Str(x.r)), KV("run", RunName(x.run))>>)
NextJson(xs) == Arr([i \in 1..Len(xs) |-> StartedJson(xs[i])])
EndedJson(x) ==
    Obj(<<KV("run", Id("r", x.run)), KV("cause", Str(x.cause)), KV("next", NextJson(x.next))>>)
EndedList(xs) == Arr([i \in 1..Len(xs) |-> EndedJson(xs[i])])

Snap(s) ==
    LET ids == Sorted({r.id : r \in s.runs})
        PerAgent(F(_)) == Arr([a \in 1..NAgents |-> F(a)])
        Starts(a) == ToString(s.starts[a][s.day])
        Slot(a) == IF s.slotrun[a] = 0 THEN "null" ELSE ToString(s.slotrun[a])
        Fails(a) == ToString(s.fails[a])
    IN <<KV("day", ToString(s.day)),
         KV("runs", Obj([i \in 1..Len(ids) |->
                KV("r" \o ToString(ids[i]), Str((CHOOSE r \in s.runs : r.id = ids[i]).phase))])),
         KV("queue", AgentList([i \in 1..Len(s.queue) |-> s.queue[i].agent])),
         KV("ready", AgentList(Sorted(s.ready))),
         KV("held", AgentList(Sorted({a \in Agents : Held(s, a)}))),
         KV("starts", PerAgent(Starts)),
         KV("slot", PerAgent(Slot)),
         KV("fails", PerAgent(Fails)),
         KV("gone", AgentList(Sorted(s.gone))),
         KV("life", ToString(s.life))>>

Args(mv) ==
    CASE mv.m = "start" ->
            <<KV("agent", Id("a", mv.agent)), KV("trigger", Str("person")), KV("port", Str(mv.port))>>
      [] mv.m \in {"hello", "wait", "resume"} -> <<KV("run", Id("r", mv.run))>>
      [] mv.m = "stop" -> <<KV("run", Id("r", mv.run)), KV("cause", Str(mv.cause))>>
      [] mv.m = "end" ->
            <<KV("run", Id("r", mv.run)),
              KV("frame", IF mv.frame = "none" THEN "null" ELSE Str(mv.frame)),
              KV("port", Str(mv.port))>>
      [] mv.m = "tick" ->
            <<KV("day", ToString(mv.day)), KV("grace", YesNo(mv.grace)), KV("port", Str(mv.port))>>
      [] mv.m = "restart" -> <<KV("day", ToString(mv.day))>>
      [] mv.m \in {"delete", "off"} -> <<KV("agent", Id("a", mv.agent))>>

Extra(mv, res) ==
    CASE mv.m = "start" -> <<KV("run", RunName(res.run))>>
      [] mv.m = "end" /\ res.r = "ok" ->
            <<KV("cause", Str(res.cause)), KV("next", NextJson(res.next))>>
      [] mv.m = "tick" ->
            <<KV("stopped", RunList(res.stopped)), KV("ended", EndedList(res.ended)),
              KV("next", NextJson(res.next)), KV("raised", AgentList(res.raised)),
              KV("started", NextJson(res.started))>>
      [] mv.m = "restart" -> <<KV("ended", EndedList(res.ended))>>
      [] mv.m = "off" -> <<KV("stopped", RunList(res.stopped))>>
      [] OTHER -> <<>>

IsEvent(x) == x.k \in {"run_started", "run_ended", "hold_set", "hold_cleared"}
IsCall(x) == x.k \in {"defer", "start_run", "start", "cancel", "kill", "reap", "set_state"}

EventJson(x) ==
    CASE x.k = "run_started" ->
            Obj(<<KV("agent", Id("a", x.agent)), KV("event", Str(x.k)), KV("run", Id("r", x.run)),
                  KV("slot", IF x.slot = 0 THEN "null" ELSE ToString(x.slot)), KV("trigger", Str(x.trig))>>)
      [] x.k = "run_ended" ->
            Obj(<<KV("agent", Id("a", x.agent)), KV("event", Str(x.k)), KV("run", Id("r", x.run)),
                  KV("cause", Str(x.cause))>>)
      [] OTHER -> Obj(<<KV("agent", Id("a", x.agent)), KV("event", Str(x.k)), KV("cause", Str(x.cause))>>)

CallJson(x) ==
    CASE x.k \in {"defer", "reap"} -> Obj(<<KV("call", Str(x.k))>>)
      [] x.k = "set_state" ->
            Obj(<<KV("call", Str(x.k)), KV("agent", Id("a", x.agent)), KV("state", Str(x.state))>>)
      [] OTHER -> Obj(<<KV("call", Str(x.k)), KV("run", Id("r", x.run))>>)

MoveJson(mv, res) ==
    LET es == SelectSeq(res.out, IsEvent)
        ps == SelectSeq(res.out, IsCall)
    IN Obj(<<KV("m", Str(mv.m)), KV("a", Obj(Args(mv))), KV("r", Str(res.r)),
             KV("e", Arr([i \in 1..Len(es) |-> EventJson(es[i])])),
             KV("p", Arr([i \in 1..Len(ps) |-> CallJson(ps[i])])),
             KV("s", Obj(Extra(mv, res) \o Snap(res.st)))>>)

Cases == Len(Directed)

TraceInit ==
    /\ st = Init0
    /\ h = InitH
    /\ w = [n |-> 1, k |-> 0, x |-> 1 + (Seed % 2147483646), acc |-> <<>>]

Play(mv, x) ==
    LET res == Step(st, mv)
    IN /\ st' = res.st
       /\ h' = Hist(h, mv, res)
       /\ w' = [w EXCEPT !.k = @ + 1, !.x = x, !.acc = Append(@, MoveJson(mv, res))]

Emit(head) ==
    /\ PrintT(Obj(head \o <<KV("moves", Arr(w.acc))>>))
    /\ st' = Init0
    /\ h' = InitH
    /\ w' = [w EXCEPT !.n = @ + 1, !.k = 0, !.acc = <<>>]

TraceNext ==
    /\ w.n <= Count + Cases
    /\ IF w.n <= Count
       THEN IF w.k = Depth
            THEN Emit(<<KV("n", ToString(w.n))>>)
            ELSE LET c == Choose(st, w.x) IN Play(c.mv, c.x)
       ELSE LET d == Directed[w.n - Count]
            IN IF w.k = Len(d.moves)
               THEN Emit(<<KV("n", ToString(w.n)), KV("case", Str(d.case))>>)
               ELSE Play(d.moves[w.k + 1], w.x)
=============================================================================
