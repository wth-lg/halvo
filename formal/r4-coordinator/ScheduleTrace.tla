--------------------------- MODULE ScheduleTrace ---------------------------
EXTENDS Schedule, TraceText

CONSTANTS Seed, Count, Depth

VARIABLE w

RefusePct == 12
PortSeq == <<"ok", "no_ack", "no_run">>
FrameSeq == <<"completed", "nothing_to_do", "declined", "handed_back", "class_mismatch",
              "refused", "limit", "effect_unknown", "failed", "none">>
KindSeq == <<"start", "hello", "wait", "resume", "stop", "end", "tick", "restart", "delete">>
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

RECURSIVE WSum(_)
WSum(ks) == IF Len(ks) = 0 THEN 0 ELSE Weight(ks[1]) + WSum(Tail(ks))

RECURSIVE WPick(_, _)
WPick(ks, t) ==
    IF Len(ks) = 1 \/ t < Weight(ks[1]) THEN ks[1]
    ELSE WPick(Tail(ks), t - Weight(ks[1]))

StartCands(s) ==
    Cat([a \in 1..NAgents |->
            IF a \in s.gone THEN <<>>
            ELSE [i \in 1..3 |-> [m |-> "start", agent |-> a, port |-> PortSeq[i]]]])

LiveCands(s, id) ==
    <<[m |-> "hello", run |-> id], [m |-> "wait", run |-> id], [m |-> "resume", run |-> id],
      [m |-> "stop", run |-> id, cause |-> "cancelled"]>>
    \o (IF (CHOOSE r \in s.runs : r.id = id).phase = "waiting"
        THEN <<[m |-> "stop", run |-> id, cause |-> "lapsed"]>>
        ELSE <<>>)
    \o [f \in 1..Len(FrameSeq) |-> [m |-> "end", run |-> id, frame |-> FrameSeq[f], port |-> "ok"]]

RunCands(s) ==
    Cat([id \in 1..s.nrun |->
            IF \E r \in s.runs : r.id = id
            THEN LiveCands(s, id)
            ELSE <<[m |-> "hello", run |-> id],
                   [m |-> "end", run |-> id, frame |-> "none", port |-> "ok"]>>])

TickCand(s, d) == [m |-> "tick", day |-> d, off |-> s.off, grace |-> TRUE, port |-> "ok"]

TickCands(s) ==
    <<TickCand(s, s.day), TickCand(s, s.day)>>
    \o (IF s.day < MaxDay THEN <<TickCand(s, s.day + 1)>> ELSE <<>>)

OtherCands(s) ==
    (IF s.life <= MaxRestarts THEN <<[m |-> "restart"]>> ELSE <<>>)
    \o Cat([a \in 1..NAgents |->
              IF s.dels < MaxDeletes /\ a \in Deletable \ s.gone
              THEN <<[m |-> "delete", agent |-> a]>>
              ELSE <<>>])

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
        x6 == Lcg(x5)
        mv == IF base.m = "tick"
              THEN [base EXCEPT !.port = port,
                                !.off = IF Draw(x5, 4) = 0 THEN OffAgents \ s.off ELSE s.off,
                                !.grace = (\A r \in s.runs : r.phase # "stopping") \/ Draw(x6, 3) # 0]
              ELSE IF base.m = "end" /\ Len(s.queue) > 0 THEN [base EXCEPT !.port = port]
              ELSE base
    IN [mv |-> mv, res |-> Step(s, mv), x |-> x6]

RunName(n) == IF n = 0 THEN "null" ELSE Id("r", n)
AgentList(xs) == Arr([i \in 1..Len(xs) |-> Id("a", xs[i])])
StartedJson(x) ==
    Obj(<<KV("agent", Id("a", x.agent)), KV("r", Str(x.r)), KV("run", RunName(x.run))>>)
NextJson(xs) == Arr([i \in 1..Len(xs) |-> StartedJson(xs[i])])
EndedJson(x) ==
    Obj(<<KV("run", Id("r", x.run)), KV("cause", Str(x.cause)), KV("next", NextJson(x.next))>>)
EndedList(xs) == Arr([i \in 1..Len(xs) |-> EndedJson(xs[i])])

Snap(s) ==
    LET ids == Sorted({r.id : r \in s.runs})
    IN <<KV("day", ToString(s.day)),
         KV("runs", Obj([i \in 1..Len(ids) |->
                KV("r" \o ToString(ids[i]), Str((CHOOSE r \in s.runs : r.id = ids[i]).phase))])),
         KV("queue", AgentList([i \in 1..Len(s.queue) |-> s.queue[i].agent])),
         KV("ready", AgentList(Sorted(s.ready))),
         KV("held", AgentList(Sorted({a \in Agents : Held(s, a)})))>>

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
            <<KV("day", ToString(mv.day)), KV("off", AgentList(Sorted(mv.off))),
              KV("grace", IF mv.grace THEN Str("yes") ELSE Str("no")), KV("port", Str(mv.port))>>
      [] mv.m = "restart" -> <<>>
      [] mv.m = "delete" -> <<KV("agent", Id("a", mv.agent))>>

Extra(mv, res) ==
    CASE mv.m = "start" -> <<KV("run", RunName(res.run))>>
      [] mv.m = "end" /\ res.r = "ok" ->
            <<KV("cause", Str(res.cause)), KV("next", NextJson(res.next))>>
      [] mv.m = "tick" ->
            <<KV("ended", EndedList(res.ended)), KV("raised", AgentList(res.raised)),
              KV("started", NextJson(res.started))>>
      [] mv.m = "restart" -> <<KV("ended", EndedList(res.ended))>>
      [] mv.m = "delete" ->
            <<KV("stopped", LET ids == Sorted(res.stopped)
                            IN Arr([i \in 1..Len(ids) |-> Id("r", ids[i])]))>>
      [] OTHER -> <<>>

MoveJson(mv, res) ==
    Obj(<<KV("m", Str(mv.m)), KV("a", Obj(Args(mv))), KV("r", Str(res.r)),
          KV("s", Obj(Extra(mv, res) \o Snap(res.st)))>>)

TraceInit ==
    /\ st = Init0
    /\ w = [n |-> 1, k |-> 0, x |-> 1 + (Seed % 2147483646), acc |-> <<>>]

TraceNext ==
    /\ w.n <= Count
    /\ IF w.k = Depth
       THEN /\ PrintT(Obj(<<KV("n", ToString(w.n)), KV("moves", Arr(w.acc))>>))
            /\ st' = Init0
            /\ w' = [w EXCEPT !.n = @ + 1, !.k = 0, !.acc = <<>>]
       ELSE LET c == Choose(st, w.x)
            IN /\ st' = c.res.st
               /\ w' = [w EXCEPT !.k = @ + 1, !.x = c.x,
                                 !.acc = Append(@, MoveJson(c.mv, c.res))]
=============================================================================
