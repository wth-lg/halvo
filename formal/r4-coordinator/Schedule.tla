------------------------------ MODULE Schedule ------------------------------
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS NAgents, NeedsPerson, NeedsModel, Slotted, Finished, OffAgents, Deletable,
          FirstSlot, Period, MaxDay, Frames, StopCauses,
          MaxLive, QueueMax, StartsPerDay, FailsToPause, CatchUpDays,
          MaxRestarts, MaxDeletes, MaxRuns

VARIABLE st

Agents == 1..NAgents
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
     today |-> [a \in Agents |-> 0],
     fails |-> [a \in Agents |-> 0],
     done |-> [a \in Agents |-> 0],
     slotrun |-> [a \in Agents |-> 0],
     sslot |-> [a \in Agents |-> 0],
     scount |-> [a \in Agents |-> 0],
     stale |-> FALSE]

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
    ELSE IF s.today[a] >= StartsPerDay THEN "budget"
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

Launch(s, a, trig, port) ==
    IF port = "no_ack" THEN [r |-> "no_service", st |-> s, run |-> 0]
    ELSE LET slot == SlotOf(s, a, trig)
             id == s.nrun + 1
             s1 == [s EXCEPT !.nrun = id,
                             !.today[a] = @ + 1,
                             !.slotrun[a] = IF slot > 0 /\ a \notin NeedsPerson THEN slot ELSE @,
                             !.sslot[a] = IF trig = "schedule" THEN slot ELSE @,
                             !.scount[a] = IF trig = "schedule"
                                           THEN (IF s.sslot[a] = slot THEN @ + 1 ELSE 1)
                                           ELSE @,
                             !.ready = @ \ {a}]
         IN IF port = "no_run"
            THEN [r |-> "no_service", st |-> Ended(s1, a, "failed", s.day), run |-> id]
            ELSE [r |-> "ok", run |-> id,
                  st |-> [s1 EXCEPT !.runs = @ \cup
                             {[id |-> id, agent |-> a, phase |-> "starting", why |-> "none",
                               trig |-> trig, day |-> IF a \in Slotted THEN s.day ELSE 0,
                               life |-> s.life,
                               ack |-> port # "no_ack", held |-> Held(s, a)]}]]

Enqueue(q, e) ==
    IF e.trigger = "person"
    THEN LET k == Cardinality({i \in DOMAIN q : q[i].trigger = "person"})
         IN SubSeq(q, 1, k) \o <<e>> \o SubSeq(q, k + 1, Len(q))
    ELSE Append(q, e)

Start(s, a, trig, port) ==
    LET d == Decide(s, a)
    IN IF d = "go" THEN Launch(s, a, trig, port)
       ELSE IF d # "wait" THEN [r |-> d, st |-> s, run |-> 0]
       ELSE IF Len(s.queue) >= QueueMax THEN [r |-> "queue_full", st |-> s, run |-> 0]
       ELSE [r |-> "queued", run |-> 0,
             st |-> [s EXCEPT !.queue = Enqueue(@, [agent |-> a, trigger |-> trig]),
                              !.ready = @ \ {a}]]

Dequeue(s, port) ==
    IF Len(s.queue) = 0 THEN [st |-> s, next |-> <<>>]
    ELSE LET h == Head(s.queue)
             s0 == [s EXCEPT !.queue = Tail(@)]
             d == Decide(s0, h.agent)
         IN IF d = "wait"
            THEN [st |-> s, next |-> <<[agent |-> h.agent, r |-> "queued", run |-> 0]>>]
            ELSE IF d = "go"
            THEN LET l == Launch(s0, h.agent, h.trigger, port)
                 IN [st |-> l.st, next |-> <<[agent |-> h.agent, r |-> l.r, run |-> l.run]>>]
            ELSE [st |-> s0, next |-> <<[agent |-> h.agent, r |-> d, run |-> 0]>>]

EndRun(s, run, cause, port) ==
    LET q == Dequeue(Ended([s EXCEPT !.runs = @ \ {run}], run.agent, cause, run.day), port)
    IN [st |-> q.st, next |-> q.next]

RECURSIVE Kill(_, _)
Kill(acc, port) ==
    LET S == {r \in acc.st.runs : r.phase = "stopping"}
    IN IF S = {} THEN acc
       ELSE LET run == CHOOSE x \in S : \A y \in S : x.id <= y.id
                e == EndRun(acc.st, run, run.why, port)
            IN Kill([st |-> e.st,
                     ended |-> Append(acc.ended, [run |-> run.id, cause |-> run.why, next |-> e.next])],
                    port)

TickAgent(s, a, port) ==
    IF a \in s.gone \/ Held(s, a) \/ LiveOf(s, a) # {} \/ Queued(s, a)
    THEN [st |-> s, raised |-> <<>>, started |-> <<>>]
    ELSE IF a \in NeedsPerson \/ a \notin s.fin
    THEN IF (Open(s, a) \/ a \notin s.fin) /\ a \notin s.ready
         THEN [st |-> [s EXCEPT !.ready = @ \cup {a}], raised |-> <<a>>, started |-> <<>>]
         ELSE [st |-> s, raised |-> <<>>, started |-> <<>>]
    ELSE IF /\ Open(s, a)
            /\ s.day - CurSlot(s.day) <= CatchUpDays
            /\ s.slotrun[a] # CurSlot(s.day)
    THEN LET x == Start(s, a, "schedule", port)
         IN [st |-> x.st, raised |-> <<>>, started |-> <<[agent |-> a, r |-> x.r, run |-> x.run]>>]
    ELSE [st |-> s, raised |-> <<>>, started |-> <<>>]

RECURSIVE TickFrom(_, _, _)
TickFrom(acc, a, port) ==
    IF a > NAgents THEN acc
    ELSE LET t == TickAgent(acc.st, a, port)
         IN TickFrom([st |-> t.st, raised |-> acc.raised \o t.raised,
                      started |-> acc.started \o t.started], a + 1, port)

Tick(s, d, o, g, port) ==
    LET s0 == [s EXCEPT !.day = d,
                        !.today = IF d # s.day THEN [a \in Agents |-> 0] ELSE @,
                        !.off = o]
        k == IF g THEN Kill([st |-> s0, ended |-> <<>>], port) ELSE [st |-> s0, ended |-> <<>>]
        t == TickFrom([st |-> k.st, raised |-> <<>>, started |-> <<>>], 1, port)
    IN [r |-> "ok", ended |-> k.ended, raised |-> t.raised, started |-> t.started,
        st |-> [t.st EXCEPT !.stale = @ \/ (g /\ \E r \in t.st.runs : r.agent \in t.st.gone)]]

RECURSIVE Recover(_, _)
Recover(s, out) ==
    IF s.runs = {} THEN [st |-> s, ended |-> out]
    ELSE LET run == CHOOSE x \in s.runs : \A y \in s.runs : x.id <= y.id
         IN Recover(Ended([s EXCEPT !.runs = @ \ {run}], run.agent, "interrupted", run.day),
                    Append(out, [run |-> run.id, cause |-> "interrupted", next |-> <<>>]))

Restart(s) ==
    LET v == Recover(s, <<>>)
    IN [r |-> "ok", ended |-> v.ended,
        st |-> [v.st EXCEPT !.life = @ + 1, !.queue = <<>>, !.ready = {}]]

RECURSIVE Without(_, _)
Without(q, a) ==
    IF Len(q) = 0 THEN <<>>
    ELSE IF Head(q).agent = a THEN Without(Tail(q), a)
    ELSE <<Head(q)>> \o Without(Tail(q), a)

Delete(s, a) ==
    [r |-> "ok",
     stopped |-> {r.id : r \in LiveOf(s, a)},
     st |-> [s EXCEPT !.gone = @ \cup {a},
                      !.dels = @ + 1,
                      !.ready = @ \ {a},
                      !.queue = Without(@, a),
                      !.runs = {IF r.agent = a THEN [r EXCEPT !.phase = "stopping", !.why = "deleted"] ELSE r : r \in @}]]

RunStep(s, mv) ==
    IF \A r \in s.runs : r.id # mv.run THEN [r |-> "state", st |-> s]
    ELSE LET run == CHOOSE r \in s.runs : r.id = mv.run
             To(p) == [r |-> "ok",
                       st |-> [s EXCEPT !.runs = (@ \ {run}) \cup {[run EXCEPT !.phase = p]}]]
             No == [r |-> "state", st |-> s]
         IN CASE mv.m = "hello" -> IF run.phase = "starting" THEN To("working") ELSE No
              [] mv.m = "wait" -> IF run.phase = "working" THEN To("waiting") ELSE No
              [] mv.m = "resume" -> IF run.phase = "waiting" THEN To("working") ELSE No
              [] mv.m = "stop" ->
                    IF run.phase \in {"starting", "working", "waiting"}
                    THEN [r |-> "ok",
                          st |-> [s EXCEPT !.runs = (@ \ {run}) \cup
                                     {[run EXCEPT !.phase = "stopping", !.why = mv.cause]}]]
                    ELSE No

End(s, mv) ==
    IF \A r \in s.runs : r.id # mv.run THEN [r |-> "state", st |-> s]
    ELSE LET run == CHOOSE r \in s.runs : r.id = mv.run
             cause == IF mv.frame # "none" THEN mv.frame
                      ELSE IF run.phase = "stopping" THEN run.why
                      ELSE "interrupted"
             e == EndRun(s, run, cause, mv.port)
         IN [r |-> "ok", st |-> e.st, cause |-> cause, next |-> e.next]

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
            d \in {s.day, s.day + 1} \cap (1..MaxDay), o \in SUBSET OffAgents,
            g \in IF \E r \in s.runs : r.phase = "stopping" THEN BOOLEAN ELSE {TRUE},
            p \in Ports}
    \cup (IF s.life <= MaxRestarts THEN {[m |-> "restart"]} ELSE {})
    \cup (IF s.dels < MaxDeletes THEN {[m |-> "delete", agent |-> a] : a \in Deletable \ s.gone} ELSE {})

Init == st = Init0
Next == \E mv \in Moves(st) : st' = Step(st, mv).st
Bound == st.nrun <= MaxRuns
View == [st EXCEPT !.ready = {},
                   !.runs = {[r EXCEPT !.phase = IF @ = "stopping" THEN @ ELSE "live"] : r \in @}]

C1 == /\ Cardinality({r \in st.runs : r.agent \in NeedsModel}) <= 1
      /\ Cardinality(st.runs) <= MaxLive
C2 == \A r \in st.runs : r.life = st.life
C3 == \A a \in Agents :
        Cardinality(LiveOf(st, a)) + Cardinality({i \in DOMAIN st.queue : st.queue[i].agent = a}) <= 1
C4 == \A r \in st.runs : r.ack /\ ~r.held
C5 == \A r \in st.runs : r.agent \in NeedsPerson => r.trig = "person"
C6 == \A a \in Agents : st.scount[a] <= 1
C7 == \A a \in Agents : st.today[a] <= StartsPerDay
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
ReachSlots == [][~(\E a \in Agents : /\ st.sslot[a] = FirstSlot
                                     /\ st'.sslot[a] = FirstSlot + Period)]_st
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
