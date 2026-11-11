------------------------------ MODULE Schedule ------------------------------
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS NAgents, NeedsPerson, NeedsModel, Slotted, Finished, OffAgents, Deletable,
          FirstSlot, Period, MaxDay, Frames, StopCauses,
          MaxLive, QueueMax, StartsPerDay, FailsToPause, CatchUpDays,
          MaxRestarts, MaxDeletes, MaxRuns

VARIABLES st, h

Agents == 1..NAgents
Days == 1..MaxDay
Triggers == {"person", "schedule"}
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
     fin |-> Finished,
     starts |-> [a \in Agents |-> [d \in Days |-> 0]],
     fails |-> [a \in Agents |-> 0],
     done |-> [a \in Agents |-> 0],
     slotrun |-> [a \in Agents |-> 0],
     scount |-> [a \in Agents |-> [d \in Days |-> 0]],
     stale |-> FALSE]

InitH ==
    [day |-> 1, life |-> 1,
     live |-> {},
     gone |-> [a \in Agents |-> "listed"],
     wait |-> [a \in Agents |-> [t \in Triggers |-> 0]],
     count |-> [a \in Agents |-> [d \in Days |-> 0]],
     done |-> Finished,
     served |-> [a \in Agents |-> 0],
     streak |-> [a \in Agents |-> 0],
     holds |-> [a \in Agents |-> {}],
     given |-> [a \in Agents |-> {}],
     sched |-> [a \in Agents |-> [d \in Days |-> 0]],
     heldstarts |-> 0, unacked |-> 0, alone |-> 0, extra |-> 0, after |-> 0, gonestarts |-> 0]

CurSlot(d) ==
    LET S == {x \in 1..d : x >= FirstSlot /\ (x - FirstSlot) % Period = 0}
    IN IF S = {} THEN 0 ELSE CHOOSE x \in S : \A y \in S : y <= x

Open(s, a) ==
    /\ a \in Slotted
    /\ CurSlot(s.day) > 0
    /\ \/ s.done[a] = 0
       \/ s.done[a] < CurSlot(s.day) - (Period \div 2)

Held(s, a) == a \in s.off \/ s.fails[a] >= FailsToPause \/ a \in s.check

LiveOf(s, a) == {r \in s.runs : r.agent = a}
Queued(s, a) == \E i \in DOMAIN s.queue : s.queue[i].agent = a

Room(s) == Cardinality(s.runs) < MaxLive
ModelFree(s, a) == a \notin NeedsModel \/ \A r \in s.runs : r.agent \notin NeedsModel

Decide(s, a) ==
    IF Held(s, a) THEN "hold"
    ELSE IF s.starts[a][s.day] >= StartsPerDay THEN "budget"
    ELSE IF LiveOf(s, a) # {} \/ Queued(s, a) THEN "state"
    ELSE IF Room(s) /\ ModelFree(s, a) THEN "go"
    ELSE "wait"

SlotOf(s, a, trig) ==
    IF trig = "schedule" THEN CurSlot(s.day)
    ELSE IF a \in s.fin /\ Open(s, a) THEN CurSlot(s.day)
    ELSE 0

Ended(s, a, cause, d) ==
    [s EXCEPT !.fin = @ \cup {a},
              !.done[a] = IF cause \in Served /\ a \in Slotted /\ d > @ THEN d ELSE @,
              !.fails[a] = IF cause \in FailureClass
                           THEN (IF @ < FailsToPause THEN @ + 1 ELSE @)
                           ELSE 0,
              !.check = IF cause = "effect_unknown" THEN @ \cup {a} ELSE @]

Launch(s, a, trig, port, q) ==
    IF port = "no_ack"
    THEN [r |-> "no_service", st |-> s, run |-> 0,
          ev |-> IF q THEN <<[k |-> "left", agent |-> a, trig |-> trig]>> ELSE <<>>]
    ELSE LET slot == SlotOf(s, a, trig)
             id == s.nrun + 1
             s1 == [s EXCEPT !.nrun = id,
                             !.starts[a][s.day] = @ + 1,
                             !.slotrun[a] = IF slot > 0 /\ a \notin NeedsPerson THEN slot ELSE @,
                             !.scount = IF trig = "schedule" THEN [@ EXCEPT ![a][slot] = @ + 1] ELSE @,
                             !.ready = @ \ {a}]
             e == [k |-> "start", run |-> id, agent |-> a, trig |-> trig, q |-> q,
                   ack |-> port # "no_ack", live |-> port # "no_run"]
         IN IF port = "no_run"
            THEN [r |-> "no_service", st |-> Ended(s1, a, "failed", s.day), run |-> id, ev |-> <<e>>]
            ELSE [r |-> "ok", run |-> id, ev |-> <<e>>,
                  st |-> [s1 EXCEPT !.runs = @ \cup
                             {[id |-> id, agent |-> a, phase |-> "starting", why |-> "none",
                               trig |-> trig, day |-> IF a \in Slotted THEN s.day ELSE 0,
                               life |-> s.life,
                               ack |-> port # "no_ack", held |-> Held(s, a)]}]]

Enqueue(q, e) ==
    IF e.trigger = "person"
    THEN LET n == Cardinality({i \in DOMAIN q : q[i].trigger = "person"})
         IN SubSeq(q, 1, n) \o <<e>> \o SubSeq(q, n + 1, Len(q))
    ELSE Append(q, e)

Start(s, a, trig, port) ==
    LET d == Decide(s, a)
    IN IF d = "go" THEN Launch(s, a, trig, port, FALSE)
       ELSE IF d # "wait" THEN [r |-> d, st |-> s, run |-> 0, ev |-> <<>>]
       ELSE IF Len(s.queue) >= QueueMax THEN [r |-> "queue_full", st |-> s, run |-> 0, ev |-> <<>>]
       ELSE [r |-> "queued", run |-> 0,
             ev |-> <<[k |-> "queued", agent |-> a, trig |-> trig]>>,
             st |-> [s EXCEPT !.queue = Enqueue(@, [agent |-> a, trigger |-> trig]),
                              !.ready = @ \ {a}]]

Dequeue(s, port) ==
    IF Len(s.queue) = 0 THEN [st |-> s, next |-> <<>>, ev |-> <<>>]
    ELSE LET head == Head(s.queue)
             s0 == [s EXCEPT !.queue = Tail(@)]
             d == Decide(s0, head.agent)
         IN IF d = "wait"
            THEN [st |-> s, next |-> <<[agent |-> head.agent, r |-> "queued", run |-> 0]>>, ev |-> <<>>]
            ELSE IF d = "go"
            THEN LET l == Launch(s0, head.agent, head.trigger, port, TRUE)
                 IN [st |-> l.st, next |-> <<[agent |-> head.agent, r |-> l.r, run |-> l.run]>>,
                     ev |-> l.ev]
            ELSE [st |-> s0, next |-> <<[agent |-> head.agent, r |-> d, run |-> 0]>>,
                  ev |-> <<[k |-> "left", agent |-> head.agent, trig |-> head.trigger]>>]

EndRun(s, run, cause, port) ==
    LET q == Dequeue(Ended([s EXCEPT !.runs = @ \ {run}], run.agent, cause, run.day), port)
    IN [st |-> q.st, next |-> q.next,
        ev |-> <<[k |-> "end", run |-> run.id, cause |-> cause]>> \o q.ev]

RECURSIVE StopEvents(_)
StopEvents(ids) ==
    IF ids = {} THEN <<>>
    ELSE LET m == CHOOSE x \in ids : \A y \in ids : x <= y
         IN <<[k |-> "stop", run |-> m]>> \o StopEvents(ids \ {m})

Due(s) == {r.id : r \in {x \in s.runs : x.phase = "stopping"}}

RECURSIVE Kill(_, _, _)
Kill(acc, due, port) ==
    LET S == {r \in acc.st.runs : r.id \in due}
    IN IF S = {} THEN acc
       ELSE LET run == CHOOSE x \in S : \A y \in S : x.id <= y.id
                e == EndRun(acc.st, run, run.why, port)
            IN Kill([st |-> e.st,
                     ended |-> Append(acc.ended, [run |-> run.id, cause |-> run.why, next |-> e.next]),
                     ev |-> acc.ev \o e.ev],
                    due \ {run.id}, port)

TickAgent(s, a, port) ==
    IF a \in s.gone \/ Held(s, a) \/ LiveOf(s, a) # {} \/ Queued(s, a)
    THEN [st |-> s, raised |-> <<>>, started |-> <<>>, ev |-> <<>>]
    ELSE IF a \in NeedsPerson \/ a \notin s.fin
    THEN IF (Open(s, a) \/ a \notin s.fin) /\ a \notin s.ready
         THEN [st |-> [s EXCEPT !.ready = @ \cup {a}], raised |-> <<a>>, started |-> <<>>, ev |-> <<>>]
         ELSE [st |-> s, raised |-> <<>>, started |-> <<>>, ev |-> <<>>]
    ELSE IF /\ Open(s, a)
            /\ s.day - CurSlot(s.day) <= CatchUpDays
            /\ s.slotrun[a] # CurSlot(s.day)
    THEN LET x == Start(s, a, "schedule", port)
         IN [st |-> x.st, raised |-> <<>>, started |-> <<[agent |-> a, r |-> x.r, run |-> x.run]>>,
             ev |-> x.ev]
    ELSE [st |-> s, raised |-> <<>>, started |-> <<>>, ev |-> <<>>]

RECURSIVE TickFrom(_, _, _)
TickFrom(acc, a, port) ==
    IF a > NAgents THEN acc
    ELSE LET t == TickAgent(acc.st, a, port)
         IN TickFrom([st |-> t.st, raised |-> acc.raised \o t.raised,
                      started |-> acc.started \o t.started, ev |-> acc.ev \o t.ev], a + 1, port)

Tick(s, d, o, g, port) ==
    LET s0 == [s EXCEPT !.day = d, !.off = o]
        k == Kill([st |-> s0, ended |-> <<>>, ev |-> <<>>], IF g THEN Due(s0) ELSE {}, port)
        t == TickFrom([st |-> k.st, raised |-> <<>>, started |-> <<>>, ev |-> <<>>], 1, port)
    IN [r |-> "ok", ended |-> k.ended, raised |-> t.raised, started |-> t.started,
        ev |-> k.ev \o t.ev,
        st |-> [t.st EXCEPT !.stale = @ \/ (g /\ \E r \in t.st.runs : r.agent \in t.st.gone)]]

RECURSIVE Recover(_, _, _)
Recover(s, out, ev) ==
    IF s.runs = {} THEN [st |-> s, ended |-> out, ev |-> ev]
    ELSE LET run == CHOOSE x \in s.runs : \A y \in s.runs : x.id <= y.id
         IN Recover(Ended([s EXCEPT !.runs = @ \ {run}], run.agent, "interrupted", run.day),
                    Append(out, [run |-> run.id, cause |-> "interrupted", next |-> <<>>]),
                    Append(ev, [k |-> "end", run |-> run.id, cause |-> "interrupted"]))

Restart(s) ==
    LET v == Recover(s, <<>>, <<>>)
    IN [r |-> "ok", ended |-> v.ended, ev |-> v.ev,
        st |-> [v.st EXCEPT !.life = @ + 1, !.queue = <<>>, !.ready = {}]]

Delete(s, a) ==
    LET hit == {r \in s.runs : r.agent = a /\ r.phase # "stopping"}
        Stays(e) == e.agent # a
        Goes(e) == e.agent = a
        out == SelectSeq(s.queue, Goes)
    IN [r |-> "ok",
        stopped |-> {r.id : r \in LiveOf(s, a)},
        ev |-> StopEvents({r.id : r \in hit})
               \o [i \in 1..Len(out) |-> [k |-> "left", agent |-> out[i].agent, trig |-> out[i].trigger]],
        st |-> [s EXCEPT !.gone = @ \cup {a},
                         !.dels = @ + 1,
                         !.ready = @ \ {a},
                         !.queue = SelectSeq(@, Stays),
                         !.runs = {IF r.agent = a THEN [r EXCEPT !.phase = "stopping", !.why = "deleted"] ELSE r : r \in @}]]

RunStep(s, mv) ==
    IF \A r \in s.runs : r.id # mv.run THEN [r |-> "state", st |-> s, ev |-> <<>>]
    ELSE LET run == CHOOSE r \in s.runs : r.id = mv.run
             To(p) == [r |-> "ok", ev |-> <<>>,
                       st |-> [s EXCEPT !.runs = (@ \ {run}) \cup {[run EXCEPT !.phase = p]}]]
             No == [r |-> "state", st |-> s, ev |-> <<>>]
         IN CASE mv.m = "hello" -> IF run.phase = "starting" THEN To("working") ELSE No
              [] mv.m = "wait" -> IF run.phase = "working" THEN To("waiting") ELSE No
              [] mv.m = "resume" -> IF run.phase = "waiting" THEN To("working") ELSE No
              [] mv.m = "stop" ->
                    IF run.phase \in {"starting", "working", "waiting"}
                    THEN [r |-> "ok", ev |-> <<[k |-> "stop", run |-> run.id]>>,
                          st |-> [s EXCEPT !.runs = (@ \ {run}) \cup
                                     {[run EXCEPT !.phase = "stopping", !.why = mv.cause]}]]
                    ELSE No

End(s, mv) ==
    IF \A r \in s.runs : r.id # mv.run THEN [r |-> "state", st |-> s, ev |-> <<>>]
    ELSE LET run == CHOOSE r \in s.runs : r.id = mv.run
             cause == IF mv.frame # "none" THEN mv.frame
                      ELSE IF run.phase = "stopping" THEN run.why
                      ELSE "interrupted"
             e == EndRun(s, run, cause, mv.port)
         IN [r |-> "ok", st |-> e.st, cause |-> cause, next |-> e.next, ev |-> e.ev]

Step(s, mv) ==
    CASE mv.m = "start" -> Start(s, mv.agent, "person", mv.port)
      [] mv.m = "tick" -> Tick(s, mv.day, mv.off, mv.grace, mv.port)
      [] mv.m = "restart" -> Restart(s)
      [] mv.m = "delete" -> Delete(s, mv.agent)
      [] mv.m = "end" -> End(s, mv)
      [] OTHER -> RunStep(s, mv)

RunMoves(s, r) ==
    (IF r.phase = "starting" THEN {[m |-> "hello", run |-> r.id]} ELSE {})
    \cup (IF r.phase = "working" THEN {[m |-> "wait", run |-> r.id]} ELSE {})
    \cup (IF r.phase = "waiting" THEN {[m |-> "resume", run |-> r.id]} ELSE {})
    \cup (IF r.phase = "stopping" THEN {}
          ELSE {[m |-> "stop", run |-> r.id, cause |-> c] : c \in StopCauses})
    \cup {[m |-> "end", run |-> r.id, frame |-> f, port |-> p] :
            f \in Frames \cup {"none"}, p \in IF Len(s.queue) = 0 THEN {"ok"} ELSE Ports}

Moves(s) ==
    {[m |-> "start", agent |-> a, port |-> p] : a \in Agents \ s.gone, p \in Ports}
    \cup UNION {RunMoves(s, r) : r \in s.runs}
    \cup {[m |-> "tick", day |-> d, off |-> o, grace |-> g, port |-> p] :
            d \in {s.day, s.day + 1} \cap Days, o \in SUBSET OffAgents,
            g \in IF \E r \in s.runs : r.phase = "stopping" THEN BOOLEAN ELSE {TRUE},
            p \in Ports}
    \cup (IF s.life <= MaxRestarts THEN {[m |-> "restart"]} ELSE {})
    \cup (IF s.dels < MaxDeletes THEN {[m |-> "delete", agent |-> a] : a \in Deletable \ s.gone} ELSE {})

HOpen(g, a) ==
    /\ a \in Slotted
    /\ CurSlot(g.day) > 0
    /\ \/ g.served[a] = 0
       \/ g.served[a] < CurSlot(g.day) - (Period \div 2)

HSlot(g, a, trig) ==
    IF trig = "schedule" THEN CurSlot(g.day)
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

HStart(acc, e) ==
    LET g == acc.h
        a == e.agent
        slot == HSlot(g, a, e.trig)
        person == e.trig = "person"
        asked == IF e.q THEN g.wait[a]["person"] > 0 ELSE acc.press
        g1 == [g EXCEPT
                 !.count[a][g.day] = IF e.ack THEN @ + 1 ELSE @,
                 !.unacked = IF e.ack THEN @ ELSE @ + 1,
                 !.heldstarts = IF g.holds[a] # {} THEN @ + 1 ELSE @,
                 !.alone = IF ~person /\ (a \in NeedsPerson \/ a \notin g.done) THEN @ + 1 ELSE @,
                 !.extra = IF person /\ ~asked THEN @ + 1 ELSE @,
                 !.gonestarts = IF g.gone[a] # "listed" THEN @ + 1 ELSE @,
                 !.after = IF ~person /\ slot \in g.given[a] THEN @ + 1 ELSE @,
                 !.sched[a] = IF ~person /\ slot > 0 THEN [@ EXCEPT ![slot] = @ + 1] ELSE @,
                 !.given[a] = IF slot > 0 THEN @ \cup {slot} ELSE @,
                 !.wait[a][e.trig] = IF e.q /\ @ > 0 THEN @ - 1 ELSE @,
                 !.live = IF e.live
                          THEN @ \cup {[id |-> e.run, agent |-> a, life |-> g.life,
                                        day |-> IF a \in Slotted THEN g.day ELSE 0,
                                        stop |-> FALSE, late |-> FALSE]}
                          ELSE @]
    IN [h |-> IF e.live THEN g1 ELSE HEnded(g1, a, "failed", g.day),
        press |-> acc.press /\ e.q]

HEnd(g, e) ==
    LET S == {r \in g.live : r.id = e.run}
    IN IF S = {} THEN g
       ELSE LET r == CHOOSE x \in S : TRUE
            IN HEnded([g EXCEPT !.live = @ \ {r}], r.agent, e.cause, r.day)

HStop(g, e) == [g EXCEPT !.live = {IF r.id = e.run THEN [r EXCEPT !.stop = TRUE] ELSE r : r \in @}]

RECURSIVE HFold(_, _, _)
HFold(acc, evs, i) ==
    IF i > Len(evs) THEN acc
    ELSE LET e == evs[i]
         IN HFold(CASE e.k = "start" -> HStart(acc, e)
                    [] e.k = "end" -> [acc EXCEPT !.h = HEnd(@, e)]
                    [] e.k = "stop" -> [acc EXCEPT !.h = HStop(@, e)]
                    [] e.k = "queued" -> [acc EXCEPT !.h.wait[e.agent][e.trig] = @ + 1, !.press = FALSE]
                    [] e.k = "left" -> [acc EXCEPT !.h.wait[e.agent][e.trig] = IF @ > 0 THEN @ - 1 ELSE @],
                  evs, i + 1)

HMove(g, mv) ==
    CASE mv.m = "tick" ->
            [g EXCEPT !.day = mv.day,
                      !.holds = [a \in Agents |-> IF a \in mv.off THEN @[a] \cup {"off"} ELSE @[a] \ {"off"}]]
      [] mv.m = "restart" -> [g EXCEPT !.life = @ + 1,
                                       !.wait = [a \in Agents |-> [t \in Triggers |-> 0]]]
      [] mv.m = "delete" -> [g EXCEPT !.gone[mv.agent] = "gone"]
      [] mv.m = "start" -> [g EXCEPT !.holds[mv.agent] = IF g.gone[mv.agent] = "listed" THEN @ \ {"failures"} ELSE @]
      [] OTHER -> g

HAfter(g0, g, mv) ==
    IF mv.m # "tick" THEN g
    ELSE [g EXCEPT !.gone = [a \in Agents |-> IF @[a] = "gone" THEN "seen" ELSE @[a]],
                   !.live = {IF mv.grace /\ (\E x \in g0.live : x.id = r.id /\ x.stop)
                             THEN [r EXCEPT !.late = TRUE]
                             ELSE r : r \in @}]

Hist(g, mv, res) ==
    HAfter(g, HFold([h |-> HMove(g, mv), press |-> mv.m = "start"], res.ev, 1).h, mv)

Init == st = Init0 /\ h = InitH
Next == \E mv \in Moves(st) : LET res == Step(st, mv) IN st' = res.st /\ h' = Hist(h, mv, res)
Bound == st.nrun <= MaxRuns
View == <<[st EXCEPT !.ready = {},
                     !.runs = {[r EXCEPT !.phase = IF @ = "stopping" THEN @ ELSE "live",
                                         !.why = IF @ = "lapsed" THEN "cancelled" ELSE @] : r \in @}], h>>

LiveRuns(a) == {r \in h.live : r.agent = a}

C1 == /\ Cardinality({r \in st.runs : r.agent \in NeedsModel}) <= 1
      /\ Cardinality(st.runs) <= MaxLive

C2 == \A r \in st.runs : r.life = st.life

C3 == \A a \in Agents :
        Cardinality(LiveOf(st, a)) + Cardinality({i \in DOMAIN st.queue : st.queue[i].agent = a}) <= 1

C4 == \A r \in st.runs : r.ack /\ ~r.held

C5 == \A r \in st.runs : r.agent \in NeedsPerson => r.trig = "person"

C6 == \A a \in Agents : \A d \in Days : st.scount[a][d] <= 1

C7 == \A a \in Agents : \A d \in Days : st.starts[a][d] <= StartsPerDay

C8 == /\ ~st.stale
      /\ \A r \in st.runs : r.agent \in st.gone => r.phase = "stopping"

ReachPerson == \A r \in st.runs : ~(r.agent \in NeedsPerson /\ r.trig = "person")
ReachAuto == \A r \in st.runs : ~(r.agent \notin NeedsPerson /\ r.trig = "schedule")
ReachHold == [][~(/\ Len(st.queue) > 0
                  /\ Head(st.queue).agent \in st.off
                  /\ Len(st'.queue) = 0
                  /\ st'.life = st.life
                  /\ st'.gone = st.gone
                  /\ st'.nrun = st.nrun
                  /\ Cardinality(st'.runs) < Cardinality(st.runs))]_st
ReachFailures == \A a \in Agents : ~(st.fails[a] >= FailsToPause /\ Decide(st, a) = "hold")
ReachSlots == [][~(\E a \in Agents : /\ st.scount[a][FirstSlot] = 1
                                     /\ st.scount[a][FirstSlot + Period] = 0
                                     /\ st'.scount[a][FirstSlot + Period] = 1)]_st
ReachCatchUp == \A a \in Agents \ NeedsPerson :
                    ~(/\ a \in st.fin
                      /\ Open(st, a)
                      /\ st.slotrun[a] # CurSlot(st.day)
                      /\ st.day - CurSlot(st.day) > CatchUpDays)
ReachQueue == [][~(/\ Len(st.queue) > 0
                   /\ \E r \in st'.runs : r.id > st.nrun /\ r.agent = Head(st.queue).agent)]_st
ReachQueueFull == \A a \in Agents : Start(st, a, "person", "ok").r # "queue_full"
ReachBudget == \A a \in Agents : Decide(st, a) # "budget"
ReachTwoLive == ~(/\ Cardinality(st.runs) = 2
                  /\ Cardinality({r \in st.runs : r.agent \in NeedsModel}) = 1)
ReachRestart == [][~(st'.life > st.life /\ st.runs # {} /\ Len(st.queue) > 0)]_st
ReachDelete == [][~(\E r \in st.runs : r.agent \notin st.gone /\ r.agent \in st'.gone)]_st
ReachDeleted == [][~(\E r \in st.runs : /\ r.agent \in st.gone
                                        /\ r.why = "deleted"
                                        /\ st'.life = st.life
                                        /\ \A x \in st'.runs : x.id # r.id)]_st
StaysLive == \A r \in st.runs : r.agent \notin st.gone

=============================================================================
