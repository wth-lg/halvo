------------------------------ MODULE Schedule ------------------------------
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS NAgents, NeedsPerson, NeedsModel, Slotted, Finished, OffAgents, Deletable,
          FirstSlot, Period, MaxDay, Frames, StopCauses,
          MaxLive, QueueMax, StartsPerDay, FailsToPause, CatchUpDays,
          MaxRestarts, MaxDeletes, MaxRuns

VARIABLES st, h

Agents == 1..NAgents
Days == 1..MaxDay
Askers == {"person", "schedule"}
Outcomes == {"completed", "nothing_to_do", "declined", "handed_back", "class_mismatch",
             "refused", "limit", "effect_unknown", "failed"}
FailureClass == {"failed", "limit", "class_mismatch", "refused", "budget", "kind"}
Served == {"completed", "nothing_to_do"}
Ports == {"ok", "no_ack", "no_run"}

ASSUME /\ NeedsPerson \subseteq Agents
       /\ NeedsModel \subseteq Agents
       /\ Slotted \subseteq Agents
       /\ Finished \subseteq Agents
       /\ OffAgents \subseteq Agents
       /\ Deletable \subseteq Agents
       /\ Frames \subseteq Outcomes
       /\ StopCauses \subseteq {"cancelled", "lapsed"}

Init0 ==
    [day |-> 1, life |-> 1, nrun |-> 0, dels |-> 0,
     runs |-> {}, queue |-> <<>>, ready |-> {}, off |-> {}, gone |-> {}, check |-> {},
     paused |-> {}, dying |-> {},
     fin |-> Finished,
     starts |-> [a \in Agents |-> [d \in Days |-> 0]],
     fails |-> [a \in Agents |-> 0],
     done |-> [a \in Agents |-> 0],
     slotrun |-> [a \in Agents |-> 0]]

InitH ==
    [day |-> 1, life |-> 1,
     live |-> {},
     gone |-> [a \in Agents |-> "listed"],
     wait |-> [a \in Agents |-> [t \in Askers |-> 0]],
     count |-> [a \in Agents |-> [d \in Days |-> 0]],
     done |-> Finished,
     served |-> [a \in Agents |-> 0],
     streak |-> [a \in Agents |-> 0],
     holds |-> [a \in Agents |-> {}],
     given |-> [a \in Agents |-> {}],
     sched |-> [a \in Agents |-> [d \in Days |-> 0]],
     heldstarts |-> 0, unacked |-> 0, alone |-> 0, extra |-> 0, after |-> 0, gonestarts |-> 0,
     wrongend |-> 0, noslot |-> 0]

CurSlot(d) ==
    LET S == {x \in 1..d : x >= FirstSlot /\ (x - FirstSlot) % Period = 0}
    IN IF S = {} THEN 0 ELSE CHOOSE x \in S : \A y \in S : y <= x

Open(s, a) ==
    /\ a \in Slotted
    /\ CurSlot(s.day) > 0
    /\ \/ s.done[a] = 0
       \/ s.done[a] < CurSlot(s.day) - (Period \div 2)

Held(s, a) == a \in s.off \/ a \in s.paused \/ a \in s.check

LiveOf(s, a) == {r \in s.runs : r.agent = a}
Queued(s, a) == \E i \in DOMAIN s.queue : s.queue[i].agent = a

Room(s) == Cardinality(s.runs) < MaxLive
ModelFree(s, a) == a \notin NeedsModel \/ \A r \in s.runs : r.agent \notin NeedsModel

Decide(s, a) ==
    IF a \in s.gone THEN "state"
    ELSE IF Held(s, a) THEN "hold"
    ELSE IF s.starts[a][s.day] >= StartsPerDay THEN "budget"
    ELSE IF LiveOf(s, a) # {} \/ Queued(s, a) THEN "state"
    ELSE IF Room(s) /\ ModelFree(s, a) THEN "go"
    ELSE "wait"

SlotOf(s, a, trig) ==
    IF trig = "schedule" THEN CurSlot(s.day)
    ELSE IF a \in s.fin /\ Open(s, a) THEN CurSlot(s.day)
    ELSE 0

SlotDue(s, a) ==
    /\ Open(s, a)
    /\ s.day - CurSlot(s.day) <= CatchUpDays
    /\ s.slotrun[a] # CurSlot(s.day)

Streak(s, a, cause) ==
    IF cause \notin FailureClass THEN 0
    ELSE IF s.fails[a] < FailsToPause THEN s.fails[a] + 1
    ELSE s.fails[a]

Ended(s, a, cause, d) ==
    LET n == Streak(s, a, cause)
    IN IF a \in s.gone THEN s ELSE
       [s EXCEPT !.fin = IF cause = "completed" THEN @ \cup {a} ELSE @,
                 !.done[a] = IF cause \in Served /\ a \in Slotted /\ d > @ THEN d ELSE @,
                 !.fails[a] = n,
                 !.paused = IF cause \in FailureClass /\ n >= FailsToPause THEN @ \cup {a} ELSE @,
                 !.check = IF cause = "effect_unknown" THEN @ \cup {a} ELSE @]

Stored(run, a, cause) == [k |-> "run_ended", run |-> run, agent |-> a, cause |-> cause]
State(a, x) == [k |-> "set_state", agent |-> a, state |-> x]

EndOut(s, run, a, cause) ==
    IF a \in s.gone THEN <<>> ELSE
    <<Stored(run, a, cause)>>
    \o (IF cause \in FailureClass /\ Streak(s, a, cause) >= FailsToPause /\ a \notin s.paused
        THEN <<[k |-> "hold_set", agent |-> a, cause |-> "failures"], State(a, "paused")>>
        ELSE <<>>)

Launch(s, a, trig, port, q) ==
    IF port = "no_ack"
    THEN [r |-> "no_service", st |-> s, run |-> 0,
          out |-> <<[k |-> "defer"]>> \o (IF q THEN <<[k |-> "left", agent |-> a]>> ELSE <<>>)]
    ELSE LET slot == SlotOf(s, a, trig)
             id == s.nrun + 1
             s1 == [s EXCEPT !.nrun = id,
                             !.starts[a][s.day] = @ + 1,
                             !.slotrun[a] = IF slot > 0 THEN slot ELSE @,
                             !.ready = @ \ {a}]
             begin == <<[k |-> "defer"],
                        [k |-> "run_started", run |-> id, agent |-> a, trig |-> trig, slot |-> slot],
                        [k |-> "start_run", run |-> id],
                        [k |-> "start", run |-> id, agent |-> a]>>
         IN IF port = "no_run"
            THEN [r |-> "no_service", st |-> Ended(s1, a, "failed", s.day), run |-> id,
                  out |-> begin \o EndOut(s1, id, a, "failed")]
            ELSE [r |-> "ok", run |-> id, out |-> begin,
                  st |-> [s1 EXCEPT !.runs = @ \cup
                             {[id |-> id, agent |-> a, phase |-> "starting", why |-> "none",
                               day |-> IF a \in Slotted THEN s.day ELSE 0]}]]

Enqueue(q, e) ==
    IF e.trigger = "person"
    THEN LET n == Cardinality({i \in DOMAIN q : q[i].trigger = "person"})
         IN SubSeq(q, 1, n) \o <<e>> \o SubSeq(q, n + 1, Len(q))
    ELSE Append(q, e)

Start(s, a, trig, port) ==
    LET d == Decide(s, a)
    IN IF d = "go" THEN Launch(s, a, trig, port, FALSE)
       ELSE IF d # "wait" THEN [r |-> d, st |-> s, run |-> 0, out |-> <<>>]
       ELSE IF Len(s.queue) >= QueueMax THEN [r |-> "queue_full", st |-> s, run |-> 0, out |-> <<>>]
       ELSE [r |-> "queued", run |-> 0,
             out |-> <<[k |-> "queued", agent |-> a]>>,
             st |-> [s EXCEPT !.queue = Enqueue(@, [agent |-> a, trigger |-> trig]),
                              !.ready = @ \ {a}]]

Press(s, a, port) ==
    LET ends == a \notin s.gone /\ a \in s.paused
        x == Start(IF a \in s.gone THEN s ELSE [s EXCEPT !.paused = @ \ {a}], a, "person", port)
    IN [x EXCEPT !.out = (IF ends THEN <<[k |-> "hold_cleared", agent |-> a, cause |-> "failures"]>> ELSE <<>>)
                         \o (IF ends /\ a \notin s.off THEN <<State(a, "active")>> ELSE <<>>)
                         \o @]

Dequeue(s, port) ==
    IF Len(s.queue) = 0 THEN [st |-> s, next |-> <<>>, out |-> <<>>]
    ELSE LET head == Head(s.queue)
             s0 == [s EXCEPT !.queue = Tail(@)]
             d0 == Decide(s0, head.agent)
             stale == head.trigger = "schedule" /\ d0 \in {"go", "wait"} /\ ~SlotDue(s0, head.agent)
             d == IF stale THEN "state" ELSE d0
         IN IF d = "wait"
            THEN [st |-> s, next |-> <<[agent |-> head.agent, r |-> "queued", run |-> 0]>>, out |-> <<>>]
            ELSE IF d = "go"
            THEN LET l == Launch(s0, head.agent, head.trigger, port, TRUE)
                 IN [st |-> l.st, next |-> <<[agent |-> head.agent, r |-> l.r, run |-> l.run]>>,
                     out |-> l.out]
            ELSE [st |-> s0, next |-> <<[agent |-> head.agent, r |-> d, run |-> 0]>>,
                  out |-> <<[k |-> "left", agent |-> head.agent]>>]

EndRun(s, run, cause, port) ==
    LET s1 == [s EXCEPT !.runs = @ \ {run}]
        q == Dequeue(Ended(s1, run.agent, cause, run.day), port)
    IN [st |-> q.st, next |-> q.next, out |-> EndOut(s1, run.id, run.agent, cause) \o q.out]

RECURSIVE Cancels(_)
Cancels(ids) ==
    IF ids = {} THEN <<>>
    ELSE LET m == CHOOSE x \in ids : \A y \in ids : x <= y
         IN <<[k |-> "cancel", run |-> m]>> \o Cancels(ids \ {m})

Due(s) == {r.id : r \in {x \in s.runs : x.phase = "stopping"}}

RECURSIVE Kill(_, _, _)
Kill(acc, due, port) ==
    IF due = {} THEN acc
    ELSE LET id == CHOOSE x \in due : \A y \in due : x <= y
             S == {r \in acc.st.runs : r.id = id}
             call == <<[k |-> "kill", run |-> id]>>
         IN IF S = {} THEN Kill([acc EXCEPT !.out = @ \o call], due \ {id}, port)
            ELSE LET run == CHOOSE x \in S : TRUE
                     e == EndRun(acc.st, run, run.why, port)
                 IN Kill([st |-> e.st,
                          ended |-> Append(acc.ended, [run |-> run.id, cause |-> run.why, next |-> e.next]),
                          out |-> acc.out \o call \o e.out],
                         due \ {id}, port)

Sweep(s) ==
    LET hit == {r \in s.runs : r.agent \in s.gone /\ r.phase # "stopping"}
        Stays(e) == e.agent \notin s.gone
        Goes(e) == e.agent \in s.gone
        gone == SelectSeq(s.queue, Goes)
    IN [stopped |-> {r.id : r \in hit},
        out |-> Cancels({r.id : r \in hit})
                \o [i \in 1..Len(gone) |-> [k |-> "left", agent |-> gone[i].agent]],
        st |-> [s EXCEPT !.runs = {IF r.agent \in s.gone
                                   THEN [r EXCEPT !.phase = "stopping", !.why = "deleted"]
                                   ELSE r : r \in @},
                         !.queue = SelectSeq(@, Stays),
                         !.ready = @ \ s.gone]]

TickAgent(s, a, port) ==
    IF a \in s.gone \/ Held(s, a) \/ LiveOf(s, a) # {} \/ Queued(s, a)
    THEN [st |-> s, raised |-> <<>>, started |-> <<>>, out |-> <<>>]
    ELSE IF a \in NeedsPerson \/ a \notin s.fin
    THEN IF (Open(s, a) \/ a \notin s.fin) /\ a \notin s.ready
         THEN [st |-> [s EXCEPT !.ready = @ \cup {a}], raised |-> <<a>>, started |-> <<>>, out |-> <<>>]
         ELSE [st |-> s, raised |-> <<>>, started |-> <<>>, out |-> <<>>]
    ELSE IF SlotDue(s, a)
    THEN LET x == Start(s, a, "schedule", port)
         IN [st |-> x.st, raised |-> <<>>, started |-> <<[agent |-> a, r |-> x.r, run |-> x.run]>>,
             out |-> x.out]
    ELSE [st |-> s, raised |-> <<>>, started |-> <<>>, out |-> <<>>]

RECURSIVE TickFrom(_, _, _)
TickFrom(acc, a, port) ==
    IF a > NAgents THEN acc
    ELSE LET t == TickAgent(acc.st, a, port)
         IN TickFrom([st |-> t.st, raised |-> acc.raised \o t.raised,
                      started |-> acc.started \o t.started, out |-> acc.out \o t.out], a + 1, port)

Tick(s, d, g, port) ==
    LET s0 == [s EXCEPT !.day = d]
        again == IF s.runs = {} THEN <<>> ELSE <<[k |-> "defer"]>>
        due == IF g THEN Due(s0) ELSE {}
        dead == IF g THEN s.dying ELSE {}
        v == Sweep(s0)
        k == Kill([st |-> [v.st EXCEPT !.dying = @ \ dead], ended |-> <<>>, out |-> <<>>], due \cup dead, port)
        i == IF Len(k.ended) = 0 /\ k.st.runs = {}
             THEN Dequeue(k.st, port)
             ELSE [st |-> k.st, next |-> <<>>, out |-> <<>>]
        t == TickFrom([st |-> i.st, raised |-> <<>>, started |-> <<>>, out |-> <<>>], 1, port)
    IN [r |-> "ok", stopped |-> v.stopped, ended |-> k.ended, next |-> i.next,
        raised |-> t.raised, started |-> t.started, st |-> t.st,
        out |-> again \o v.out \o k.out \o i.out \o t.out]

RECURSIVE Recover(_, _, _)
Recover(s, ended, out) ==
    IF s.runs = {} THEN [st |-> s, ended |-> ended, out |-> out]
    ELSE LET run == CHOOSE x \in s.runs : \A y \in s.runs : x.id <= y.id
             s1 == [s EXCEPT !.runs = @ \ {run}]
         IN Recover(Ended(s1, run.agent, "interrupted", run.day),
                    Append(ended, [run |-> run.id, cause |-> "interrupted", next |-> <<>>]),
                    out \o EndOut(s1, run.id, run.agent, "interrupted"))

Restart(s, d) ==
    LET v == Recover(s, <<>>, <<[k |-> "reap"]>>)
    IN [r |-> "ok", ended |-> v.ended, out |-> v.out,
        st |-> [v.st EXCEPT !.life = @ + 1, !.day = d, !.queue = <<>>, !.ready = {}, !.dying = {}]]

Delete(s, a) ==
    [r |-> "ok", out |-> <<>>,
     st |-> [s EXCEPT !.gone = @ \cup {a}, !.dels = @ + 1]]

Off(s, a) ==
    LET hit == {r \in s.runs : r.agent = a /\ r.phase # "stopping"}
    IN [r |-> "ok", stopped |-> {r.id : r \in hit},
        out |-> <<[k |-> "hold_set", agent |-> a, cause |-> "person"]>>
                \o Cancels({r.id : r \in hit}) \o <<State(a, "paused")>>,
        st |-> [s EXCEPT !.off = @ \cup {a},
                         !.runs = (@ \ hit) \cup {[r EXCEPT !.phase = "stopping", !.why = "cancelled"] : r \in hit}]]

RunStep(s, mv) ==
    IF \A r \in s.runs : r.id # mv.run THEN [r |-> "state", st |-> s, out |-> <<>>]
    ELSE LET run == CHOOSE r \in s.runs : r.id = mv.run
             To(p) == [r |-> "ok", out |-> <<>>,
                       st |-> [s EXCEPT !.runs = (@ \ {run}) \cup {[run EXCEPT !.phase = p]}]]
             No == [r |-> "state", st |-> s, out |-> <<>>]
         IN CASE mv.m = "hello" -> IF run.phase = "starting" THEN To("working") ELSE No
              [] mv.m = "wait" -> IF run.phase = "working" THEN To("waiting") ELSE No
              [] mv.m = "resume" -> IF run.phase = "waiting" THEN To("working") ELSE No
              [] mv.m = "stop" ->
                    IF run.phase \in {"starting", "working", "waiting"}
                    THEN [r |-> "ok", out |-> <<[k |-> "cancel", run |-> run.id]>>,
                          st |-> [s EXCEPT !.runs = (@ \ {run}) \cup
                                     {[run EXCEPT !.phase = "stopping", !.why = mv.cause]}]]
                    ELSE No

End(s, mv) ==
    IF \A r \in s.runs : r.id # mv.run THEN [r |-> "state", st |-> s, out |-> <<>>]
    ELSE LET run == CHOOSE r \in s.runs : r.id = mv.run
             cause == IF run.phase = "stopping" THEN run.why
                      ELSE IF mv.frame # "none" THEN mv.frame
                      ELSE "interrupted"
             e == EndRun(s, run, cause, mv.port)
         IN [r |-> "ok", cause |-> cause, next |-> e.next, out |-> e.out,
             st |-> IF run.phase = "stopping" THEN [e.st EXCEPT !.dying = @ \cup {run.id}] ELSE e.st]

Step(s, mv) ==
    CASE mv.m = "start" -> Press(s, mv.agent, mv.port)
      [] mv.m = "tick" -> Tick(s, mv.day, mv.grace, mv.port)
      [] mv.m = "restart" -> Restart(s, mv.day)
      [] mv.m = "delete" -> Delete(s, mv.agent)
      [] mv.m = "off" -> Off(s, mv.agent)
      [] mv.m = "end" -> End(s, mv)
      [] OTHER -> RunStep(s, mv)

RunMoves(s, r) ==
    (IF r.phase = "starting" THEN {[m |-> "hello", run |-> r.id]} ELSE {})
    \cup (IF r.phase = "working" THEN {[m |-> "wait", run |-> r.id]} ELSE {})
    \cup (IF r.phase = "waiting" THEN {[m |-> "resume", run |-> r.id]} ELSE {})
    \cup (IF r.phase = "stopping" THEN {}
          ELSE {[m |-> "stop", run |-> r.id, cause |-> c] : c \in StopCauses})
    \cup {[m |-> "end", run |-> r.id, frame |-> f, port |-> p] :
            f \in IF r.phase = "stopping" THEN {"none"} ELSE Frames,
            p \in IF Len(s.queue) = 0 THEN {"ok"} ELSE Ports}

Moves(s) ==
    {[m |-> "start", agent |-> a, port |-> p] : a \in Agents, p \in Ports}
    \cup UNION {RunMoves(s, r) : r \in s.runs}
    \cup {[m |-> "tick", day |-> d, grace |-> g, port |-> p] :
            d \in {s.day, s.day + 1} \cap Days,
            g \in IF s.dying # {} \/ \E r \in s.runs : r.phase = "stopping" THEN BOOLEAN ELSE {TRUE},
            p \in Ports}
    \cup (IF s.life <= MaxRestarts
          THEN {[m |-> "restart", day |-> d] : d \in {s.day, s.day + 1} \cap Days}
          ELSE {})
    \cup (IF s.dels < MaxDeletes THEN {[m |-> "delete", agent |-> a] : a \in Deletable \ s.gone} ELSE {})
    \cup {[m |-> "off", agent |-> a] : a \in OffAgents \ s.off}

PortOf(mv) == IF mv.m \in {"start", "tick", "end"} THEN mv.port ELSE "ok"

HOpen(g, a) ==
    /\ a \in Slotted
    /\ CurSlot(g.day) > 0
    /\ \/ g.served[a] = 0
       \/ g.served[a] < CurSlot(g.day) - (Period \div 2)

HSlot(g, a, person) ==
    IF ~person THEN CurSlot(g.day)
    ELSE IF a \in g.done /\ HOpen(g, a) THEN CurSlot(g.day)
    ELSE 0

HEnded(g, a, cause, d) ==
    LET n == IF cause \notin FailureClass THEN 0
             ELSE IF g.streak[a] < FailsToPause THEN g.streak[a] + 1
             ELSE g.streak[a]
    IN [g EXCEPT !.done = IF cause = "completed" THEN @ \cup {a} ELSE @,
                 !.served[a] = IF cause \in Served /\ a \in Slotted /\ d > @ THEN d ELSE @,
                 !.streak[a] = n,
                 !.holds[a] = (IF cause \in FailureClass /\ n >= FailsToPause THEN @ \cup {"failures"} ELSE @)
                              \cup (IF cause = "effect_unknown" THEN {"check"} ELSE {})]

HLess(w) ==
    IF w["person"] > 0 THEN [w EXCEPT !["person"] = @ - 1]
    ELSE IF w["schedule"] > 0 THEN [w EXCEPT !["schedule"] = @ - 1]
    ELSE w

HDue(g, a) ==
    /\ HOpen(g, a)
    /\ g.day - CurSlot(g.day) <= CatchUpDays
    /\ CurSlot(g.day) \notin g.given[a]

HStopped(g, ids, why) ==
    [g EXCEPT !.live = {IF r.id \in ids /\ ~r.stop THEN [r EXCEPT !.stop = TRUE, !.why = why] ELSE r : r \in @}]

HMove(g, mv) ==
    CASE mv.m = "tick" ->
            [g EXCEPT !.day = mv.day,
                      !.live = {IF r.stop /\ g.gone[r.agent] # "listed" THEN [r EXCEPT !.why = "deleted"] ELSE r :
                                   r \in @}]
      [] mv.m = "restart" -> [g EXCEPT !.life = @ + 1, !.day = mv.day,
                                       !.wait = [a \in Agents |-> [t \in Askers |-> 0]]]
      [] mv.m = "delete" -> [g EXCEPT !.gone[mv.agent] = "gone"]
      [] mv.m = "off" ->
            [HStopped(g, {r.id : r \in {x \in g.live : x.agent = mv.agent}}, "cancelled")
                EXCEPT !.holds[mv.agent] = @ \cup {"off"}]
      [] mv.m = "start" -> [g EXCEPT !.holds[mv.agent] = IF g.gone[mv.agent] = "listed" THEN @ \ {"failures"} ELSE @]
      [] mv.m = "stop" -> HStopped(g, {mv.run}, mv.cause)
      [] OTHER -> g

HOwes(r, cause) == [id |-> r.id, agent |-> r.agent, day |-> r.day, cause |-> cause]

HLeft(acc, S, reaped) ==
    [acc EXCEPT !.h.live = @ \ S,
                !.owed = @ \cup {HOwes(r, IF reaped THEN "interrupted" ELSE IF r.stop THEN r.why ELSE "none") :
                                    r \in {x \in S : acc.h.gone[x.agent] = "listed"}}]

HStarted(acc, e, mv) ==
    IF PortOf(mv) = "no_ack" THEN acc
    ELSE LET g == acc.h
             a == e.agent
             direct == acc.press /\ mv.agent = a
             person == direct \/ g.wait[a]["person"] > 0
             slot == HSlot(g, a, person)
         IN [acc EXCEPT
               !.press = FALSE,
               !.acked = @ \cup {e.run},
               !.h = [g EXCEPT
                        !.count[a][g.day] = @ + 1,
                        !.heldstarts = IF g.holds[a] # {} THEN @ + 1 ELSE @,
                        !.alone = IF ~person /\ (a \in NeedsPerson \/ a \notin g.done) THEN @ + 1 ELSE @,
                        !.extra = IF mv.m = "start" /\ ~direct THEN @ + 1 ELSE @,
                        !.noslot = IF person \/ HDue(g, a) THEN @ ELSE @ + 1,
                        !.gonestarts = IF g.gone[a] # "listed" THEN @ + 1 ELSE @,
                        !.after = IF ~person /\ slot \in g.given[a] THEN @ + 1 ELSE @,
                        !.sched[a] = IF ~person /\ slot > 0 THEN [@ EXCEPT ![slot] = @ + 1] ELSE @,
                        !.given[a] = IF slot > 0 THEN @ \cup {slot} ELSE @,
                        !.wait[a] = IF direct THEN @ ELSE HLess(@)]]

HSpawned(acc, e, mv) ==
    LET g == [acc.h EXCEPT !.unacked = IF e.run \in acc.acked THEN @ ELSE @ + 1]
        r == [id |-> e.run, agent |-> e.agent, life |-> g.life,
              day |-> IF e.agent \in Slotted THEN g.day ELSE 0,
              stop |-> FALSE, late |-> FALSE, why |-> "none"]
    IN IF PortOf(mv) = "no_run"
       THEN [acc EXCEPT !.h = g, !.owed = @ \cup {HOwes([r EXCEPT !.day = g.day], "failed")}]
       ELSE [acc EXCEPT !.h = [g EXCEPT !.live = @ \cup {r}]]

HStored(acc, e) ==
    LET O == {o \in acc.owed : o.id = e.run}
        L == {r \in acc.h.live : r.id = e.run}
    IN IF O # {}
       THEN LET o == CHOOSE x \in O : TRUE
            IN [acc EXCEPT !.owed = @ \ {o},
                           !.h = [HEnded(acc.h, o.agent, e.cause, o.day)
                                     EXCEPT !.wrongend = IF e.cause = o.cause THEN @ ELSE @ + 1]]
       ELSE IF L # {}
       THEN LET r == CHOOSE x \in L : TRUE
            IN [acc EXCEPT !.h = [HEnded(acc.h, r.agent, e.cause, r.day) EXCEPT !.wrongend = @ + 1]]
       ELSE [acc EXCEPT !.h.wrongend = @ + 1]

HOut(acc, e, mv) ==
    CASE e.k = "run_started" -> HStarted(acc, e, mv)
      [] e.k = "start" -> HSpawned(acc, e, mv)
      [] e.k = "run_ended" -> HStored(acc, e)
      [] e.k = "kill" -> HLeft(acc, {r \in acc.h.live : r.id = e.run}, FALSE)
      [] e.k = "reap" -> HLeft(acc, {r \in acc.h.live : r.life < acc.h.life}, TRUE)
      [] e.k = "cancel" ->
            [acc EXCEPT !.h = HStopped(@, {e.run}, IF mv.m = "tick" THEN "deleted" ELSE "cancelled")]
      [] e.k = "queued" ->
            [acc EXCEPT !.h.wait[e.agent][IF acc.press /\ mv.agent = e.agent THEN "person" ELSE "schedule"] = @ + 1,
                        !.press = FALSE]
      [] e.k = "left" -> [acc EXCEPT !.h.wait[e.agent] = HLess(@)]
      [] OTHER -> acc

RECURSIVE HFold(_, _, _, _)
HFold(acc, out, i, mv) == IF i > Len(out) THEN acc ELSE HFold(HOut(acc, out[i], mv), out, i + 1, mv)

HAfter(g0, g, mv) ==
    IF mv.m # "tick" THEN g
    ELSE [g EXCEPT !.gone = [a \in Agents |-> IF @[a] = "gone" THEN "seen" ELSE @[a]],
                   !.live = {IF mv.grace /\ (\E x \in g0.live : x.id = r.id /\ x.stop)
                             THEN [r EXCEPT !.late = TRUE]
                             ELSE r : r \in @}]

Hist(g, mv, res) ==
    LET g1 == HMove(g, mv)
        ends == IF mv.m = "end" THEN {r \in g1.live : r.id = mv.run} ELSE {}
        owed == {HOwes(r, IF r.stop THEN r.why ELSE IF mv.frame # "none" THEN mv.frame ELSE "interrupted") :
                    r \in {x \in ends : g1.gone[x.agent] = "listed"}}
        acc == HFold([h |-> [g1 EXCEPT !.live = @ \ ends], owed |-> owed, press |-> mv.m = "start", acked |-> {}],
                     res.out, 1, mv)
    IN HAfter(g, [acc.h EXCEPT !.wrongend = @ + Cardinality(acc.owed)], mv)

Init == st = Init0 /\ h = InitH
Next == \E mv \in Moves(st) : LET res == Step(st, mv) IN st' = res.st /\ h' = Hist(h, mv, res)
Bound == st.nrun <= MaxRuns
View == <<[st EXCEPT !.ready = {}, !.dying = {},
                     !.runs = {[r EXCEPT !.phase = IF @ = "stopping" THEN @ ELSE "live"] : r \in @}],
          h>>

LiveRuns(a) == {r \in h.live : r.agent = a}

C1 == /\ Cardinality({r \in h.live : r.agent \in NeedsModel}) <= 1
      /\ Cardinality(h.live) <= MaxLive
C2 == \A r \in h.live : r.life = h.life
C3 == \A a \in Agents :
        Cardinality(LiveRuns(a)) + h.wait[a]["person"] + h.wait[a]["schedule"] <= 1
C4 == h.heldstarts = 0 /\ h.unacked = 0
C5 == h.alone = 0 /\ h.extra = 0
C6 == /\ h.after = 0
      /\ \A a \in Agents : \A d \in Days : h.sched[a][d] <= 1
C7 == \A a \in Agents : \A d \in Days : h.count[a][d] <= StartsPerDay
C8 == /\ h.gonestarts = 0
      /\ \A r \in h.live : h.gone[r.agent] = "seen" => r.stop /\ ~r.late
C9 == \A r \in h.live : ~r.late
C10 == h.wrongend = 0
C11 == h.noslot = 0

vars == <<st, h>>
Stopping(c) == {r \in st.runs : r.phase = "stopping" /\ r.why = c}
KilledAt(r) == st'.day # st.day /\ st'.life = st.life /\ \A x \in st'.runs : x.id # r.id

ReachPerson == \A r \in h.live : r.agent \notin NeedsPerson
ReachAuto == \A a \in Agents \ NeedsPerson : \A d \in Days : h.sched[a][d] = 0
ReachDeletableAuto == \A a \in Deletable : \A d \in Days : h.sched[a][d] = 0
ReachFailures == \A a \in Agents : "failures" \notin h.holds[a]
ReachFailuresCleared == [][~(\E a \in Agents : "failures" \in h.holds[a] /\ "failures" \notin h'.holds[a])]_vars
ReachSlots == \A a \in Agents : ~(h.sched[a][FirstSlot] = 1 /\ h.sched[a][FirstSlot + Period] = 1)
ReachCatchUp == \A a \in Agents \ NeedsPerson :
                    ~(/\ a \in st.fin
                      /\ Open(st, a)
                      /\ st.slotrun[a] # CurSlot(st.day)
                      /\ st.day - CurSlot(st.day) > CatchUpDays)
ReachQueue == [][~(\E a \in Agents : /\ h.wait[a]["person"] + h.wait[a]["schedule"] = 1
                                     /\ \E r \in h'.live : r.agent = a /\ r.id > st.nrun)]_vars
ReachQueueTwo == Len(st.queue) < 2
ReachQueueOrder == ~(Len(st.queue) = 2 /\ st.queue[1].trigger = "person" /\ st.queue[2].trigger = "schedule")
ReachQueueFull == \A a \in Agents : Start(st, a, "person", "ok").r # "queue_full"
ReachQueueIdle == [][~(/\ st.runs = {}
                       /\ Len(st.queue) > 0
                       /\ st'.life = st.life
                       /\ st'.nrun > st.nrun
                       /\ Len(st'.queue) < Len(st.queue))]_vars
ReachBudget == \A a \in Agents : Decide(st, a) # "budget"
ReachTwoLive == ~(/\ Cardinality(h.live) = 2
                  /\ Cardinality({r \in h.live : r.agent \in NeedsModel}) = 1)
ReachRestart == [][~(st'.life > st.life /\ st.runs # {} /\ Len(st.queue) > 0)]_vars
ReachRestartDay == [][~(st'.life > st.life /\ st'.day > st.day)]_vars
ReachRestartStopping == [][~(st'.life > st.life /\ \E r \in st.runs : r.phase = "stopping")]_vars
ReachGoneStart == \A a \in Agents : ~(a \in st.gone /\ Press(st, a, "ok").r = "state")
ReachGoneSeen == \A r \in h.live : ~(h.gone[r.agent] = "seen" /\ r.stop)
ReachGoneQueued == [][~(\E a \in Agents : /\ h.gone[a] = "gone"
                                          /\ h.wait[a]["person"] + h.wait[a]["schedule"] = 1
                                          /\ h'.gone[a] = "seen"
                                          /\ h'.wait[a]["person"] + h'.wait[a]["schedule"] = 0)]_vars
ReachOffLive == [][~(\E r \in h.live : /\ ~r.stop
                                       /\ "off" \notin h.holds[r.agent]
                                       /\ "off" \in h'.holds[r.agent])]_vars
ReachOffQueued == [][~(\E a \in Agents : /\ "off" \in h.holds[a]
                                         /\ h.wait[a]["person"] = 1
                                         /\ h'.wait[a]["person"] = 0
                                         /\ h'.life = h.life
                                         /\ st'.nrun = st.nrun)]_vars
ReachKillCancelled == [][~(\E r \in Stopping("cancelled") : r.agent \notin st.off /\ KilledAt(r))]_vars
ReachKillOff == [][~(\E r \in Stopping("cancelled") : r.agent \in st.off /\ KilledAt(r))]_vars
ReachKillLapsed == [][~(\E r \in Stopping("lapsed") : KilledAt(r))]_vars
ReachKillDeleted == [][~(\E r \in Stopping("deleted") : KilledAt(r))]_vars
ReachKillEnded == [][~(st.dying # {} /\ st'.dying = {} /\ st'.life = st.life)]_vars
ReachRecause == [][~(\E r \in Stopping("cancelled") : \E x \in st'.runs : x.id = r.id /\ x.why = "deleted")]_vars
ReachFailedFrame == [][~(\E a \in Agents : h'.streak[a] > h.streak[a] /\ h'.count[a] = h.count[a])]_vars
ReachFailedStart == [][~(\E a \in Agents : h'.streak[a] > h.streak[a] /\ h'.count[a] # h.count[a])]_vars
ReachQueuedStale == [][~(\E a \in Agents : /\ h.wait[a]["schedule"] = 1
                                           /\ h'.wait[a]["schedule"] = 0
                                           /\ h'.life = h.life
                                           /\ h'.count[a] = h.count[a]
                                           /\ Decide(st', a) \in {"go", "wait"}
                                           /\ ~SlotDue(st', a))]_vars

StaysLive == \A r \in h.live : h.gone[r.agent] = "listed"
=============================================================================
