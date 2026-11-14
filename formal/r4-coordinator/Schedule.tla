------------------------------ MODULE Schedule ------------------------------
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS NAgents, NeedsPerson, NeedsModel, Slotted, Finished, OffAgents, Deletable,
          FirstSlot, Period, MaxDay, Frames, StopCauses,
          MaxLive, QueueMax, StartsPerDay, FailsToPause, CatchUpDays,
          MaxRestarts, MaxDeletes, MaxRuns, Lefts, MaxAlive, StoreWaits, Ports

VARIABLES st, h

Agents == 1..NAgents
Days == 1..MaxDay
Askers == {"person", "schedule"}
Outcomes == {"completed", "nothing_to_do", "declined", "handed_back", "class_mismatch",
             "refused", "limit", "effect_unknown", "failed"}
FailureClass == {"failed", "limit", "class_mismatch", "refused", "budget", "kind"}
Served == {"completed", "nothing_to_do"}

ASSUME /\ NeedsPerson \subseteq Agents
       /\ NeedsModel \subseteq Agents
       /\ Slotted \subseteq Agents
       /\ Finished \subseteq Agents
       /\ OffAgents \subseteq Agents
       /\ Deletable \subseteq Agents
       /\ Frames \subseteq Outcomes
       /\ StopCauses \subseteq {"cancelled", "lapsed"}
       /\ Lefts \subseteq BOOLEAN
       /\ Lefts # {}
       /\ StoreWaits \in BOOLEAN
       /\ Ports \subseteq {"ok", "no_ack", "no_store", "no_run"}
       /\ "ok" \in Ports

NoFacts == [open |-> [a \in Agents |-> 0], unknown |-> [a \in Agents |-> 0]]
NoMark == [run |-> 0, dead |-> FALSE, left |-> FALSE, sent |-> FALSE, by |-> "none"]

Init0 ==
    [day |-> 1, life |-> 1, nrun |-> 0, dels |-> 0,
     runs |-> {}, queue |-> <<>>, ready |-> {}, off |-> {}, gone |-> {}, lost |-> {}, cleared |-> {},
     paused |-> {}, dying |-> {}, owing |-> {}, f |-> NoFacts,
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
     store |-> [a \in Agents |-> NoMark],
     unknown |-> [a \in Agents |-> 0],
     answered |-> {}, alive |-> {},
     heldstarts |-> 0, unacked |-> 0, alone |-> 0, extra |-> 0, after |-> 0, gonestarts |-> 0,
     wrongend |-> 0, noslot |-> 0, openstarts |-> 0, wrongsend |-> 0, forgotten |-> 0]

Facts(g) == [open |-> [a \in Agents |-> g.store[a].run], unknown |-> g.unknown]

CurSlot(d) ==
    LET S == {x \in 1..d : x >= FirstSlot /\ (x - FirstSlot) % Period = 0}
    IN IF S = {} THEN 0 ELSE CHOOSE x \in S : \A y \in S : y <= x

Open(s, a) ==
    /\ a \in Slotted
    /\ CurSlot(s.day) > 0
    /\ \/ s.done[a] = 0
       \/ s.done[a] < CurSlot(s.day) - (Period \div 2)

Checked(s, a) == s.f.unknown[a] # 0 /\ s.f.unknown[a] \notin s.cleared
Held(s, a) == a \notin s.lost /\ (a \in s.off \/ a \in s.paused \/ Checked(s, a))
Unready(s, read) == [s EXCEPT !.ready = {a \in @ : a \notin read \/ a \in s.gone \/ ~Checked(s, a)}]

LiveOf(s, a) == {r \in s.runs : r.agent = a}
Queued(s, a) == \E i \in DOMAIN s.queue : s.queue[i].agent = a

Room(s) == Cardinality(s.runs) < MaxLive
ModelFree(s, a) == a \notin NeedsModel \/ \A r \in s.runs : r.agent \notin NeedsModel

Decide(s, a) ==
    IF a \in s.gone THEN "state"
    ELSE IF Held(s, a) THEN "hold"
    ELSE IF s.starts[a][s.day] >= StartsPerDay THEN "budget"
    ELSE IF LiveOf(s, a) # {} \/ Queued(s, a) THEN "state"
    ELSE IF s.f.open[a] # 0 THEN "state"
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
                 !.paused = IF cause \in FailureClass /\ n >= FailsToPause THEN @ \cup {a} ELSE @]

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
                             !.f = [@ EXCEPT !.open[a] = id, !.unknown[a] = 0],
                             !.ready = @ \ {a}]
             asked == <<[k |-> "defer"],
                        [k |-> "run_started", run |-> id, agent |-> a, trig |-> trig, slot |-> slot],
                        [k |-> "start_run", run |-> id]>>
             begin == asked \o <<[k |-> "start", run |-> id, agent |-> a]>>
             owe == <<[k |-> "end_run", run |-> id]>>
         IN IF port \in {"no_store", "no_run"}
            THEN [r |-> "no_service", st |-> Ended([s1 EXCEPT !.owing = @ \cup {id}], a, "failed", s.day), run |-> id,
                  out |-> (IF port = "no_store" THEN asked ELSE begin) \o EndOut(s1, id, a, "failed")
                          \o (IF port = "no_store" THEN owe ELSE <<>>)
                          \o (IF port = "no_run" THEN owe ELSE <<>>)]
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

Pressed(s, a, port) ==
    LET ends == a \notin s.gone /\ a \in s.paused
        x == Start(IF a \in s.gone THEN s ELSE [s EXCEPT !.paused = @ \ {a}], a, "person", port)
    IN [x EXCEPT !.out = (IF ends THEN <<[k |-> "hold_cleared", agent |-> a, cause |-> "failures"]>> ELSE <<>>)
                         \o (IF ends /\ a \notin s.off THEN <<State(a, "active")>> ELSE <<>>)
                         \o @]

Press(s, a, port) ==
    IF a \in s.lost THEN [r |-> "unknown_agent", st |-> s, run |-> 0, out |-> <<>>]
    ELSE LET x == Pressed(s, a, port) IN [x EXCEPT !.st = Unready(@, {a})]

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

Closed(s, run, unk) ==
    [s EXCEPT !.f = [@ EXCEPT !.open[run.agent] = 0, !.unknown[run.agent] = IF unk THEN run.id ELSE 0]]

EndRun(s, run, cause, port, fin, unk) ==
    LET s1 == [(IF fin THEN Closed(s, run, unk) ELSE s) EXCEPT !.runs = @ \ {run}]
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
                     e == EndRun(acc.st, run, run.why, port, FALSE, FALSE)
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
    IF a \in s.gone \/ Held(s, a) \/ LiveOf(s, a) # {} \/ Queued(s, a) \/ a \in s.lost
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

RECURSIVE Sends(_)
Sends(ids) ==
    IF ids = {} THEN <<>>
    ELSE LET m == CHOOSE x \in ids : \A y \in ids : x <= y
         IN <<[k |-> "end_run", run |-> m]>> \o Sends(ids \ {m})

Owed(s) == {id \in s.owing : \E a \in Agents : s.f.open[a] = id}
Kept(s, pay) == {id \in pay : \E a \in Agents \ s.gone : s.f.open[a] = id}

Tick(s, d, g, port) ==
    LET s0 == [s EXCEPT !.day = d]
        again == IF s.runs = {} THEN <<>> ELSE <<[k |-> "defer"]>>
        due == IF g THEN Due(s0) ELSE {}
        dead == IF g THEN s.dying ELSE {}
        v == Sweep(s0)
        k == Kill([st |-> [v.st EXCEPT !.dying = @ \ dead], ended |-> <<>>, out |-> <<>>], due \cup dead, port)
        pay == Owed(k.st)
        paid == [k.st EXCEPT !.owing = Kept(k.st, pay)]
        i == IF Len(k.ended) = 0 /\ k.st.runs = {}
             THEN Dequeue(paid, port)
             ELSE [st |-> paid, next |-> <<>>, out |-> <<>>]
        t == TickFrom([st |-> i.st, raised |-> <<>>, started |-> <<>>, out |-> <<>>], 1, port)
    IN [r |-> "ok", stopped |-> v.stopped, ended |-> k.ended, next |-> i.next,
        raised |-> t.raised, started |-> t.started, st |-> Unready(t.st, Agents),
        out |-> again \o v.out \o k.out \o Sends(pay) \o i.out \o t.out]

RECURSIVE Recover(_, _, _)
Recover(s, ended, out) ==
    IF s.runs = {} THEN [st |-> s, ended |-> ended, out |-> out]
    ELSE LET run == CHOOSE x \in s.runs : \A y \in s.runs : x.id <= y.id
             s1 == [s EXCEPT !.runs = @ \ {run}]
         IN Recover(Ended(s1, run.agent, "interrupted", run.day),
                    IF run.agent \in s.gone THEN ended
                    ELSE Append(ended, [run |-> run.id, cause |-> "interrupted", next |-> <<>>]),
                    out \o EndOut(s1, run.id, run.agent, "interrupted"))

Marks(s, alive) == {s.f.open[a] : a \in {x \in Agents \ s.gone : s.f.open[x] # 0}} \ alive

Restart(s, d, alive) ==
    LET v == Recover(s, <<>>, <<[k |-> "reap"]>>)
        owed == Marks(s, alive)
    IN [r |-> "ok", ended |-> v.ended, out |-> v.out \o Sends(owed),
        st |-> [v.st EXCEPT !.life = @ + 1, !.day = d, !.queue = <<>>, !.ready = {}, !.dying = {},
                            !.lost = @ \cup s.gone, !.gone = {}, !.off = @ \ s.gone, !.paused = @ \ s.gone,
                            !.owing = owed]]

Check(s, a) ==
    IF a \in s.gone \/ ~Checked(s, a) THEN [r |-> "state", st |-> s, out |-> <<>>]
    ELSE [r |-> "ok",
          out |-> <<[k |-> "hold_cleared", agent |-> a, cause |-> "check", run |-> s.f.unknown[a]]>>,
          st |-> [Unready(s, {a}) EXCEPT !.cleared = @ \cup {s.f.unknown[a]}]]

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
             e == EndRun(s, run, cause, mv.port, mv.frame # "none", mv.frame = "effect_unknown")
         IN [r |-> "ok", cause |-> cause, next |-> e.next, out |-> e.out,
             st |-> Unready(IF run.phase = "stopping" THEN [e.st EXCEPT !.dying = @ \cup {run.id}] ELSE e.st,
                            Agents)]

Read(s, mv) ==
    CASE mv.m = "start" -> Press(s, mv.agent, mv.port)
      [] mv.m = "tick" -> Tick(s, mv.day, mv.grace, mv.port)
      [] mv.m = "restart" -> Restart(s, mv.day, mv.alive)
      [] mv.m = "delete" -> Delete(s, mv.agent)
      [] mv.m = "off" -> Off(s, mv.agent)
      [] mv.m = "end" -> End(s, mv)
      [] mv.m = "check" -> Check(s, mv.agent)
      [] mv.m = "ack" -> [r |-> "ok", st |-> s, out |-> <<>>]
      [] OTHER -> RunStep(s, mv)

Step(s, mv, f) == [Read([s EXCEPT !.f = f], mv) EXCEPT !.st.f = NoFacts]

RunMoves(s, r) ==
    (IF r.phase = "starting" THEN {[m |-> "hello", run |-> r.id]} ELSE {})
    \cup (IF r.phase = "working" THEN {[m |-> "wait", run |-> r.id]} ELSE {})
    \cup (IF r.phase = "waiting" THEN {[m |-> "resume", run |-> r.id]} ELSE {})
    \cup (IF r.phase = "stopping" THEN {}
          ELSE {[m |-> "stop", run |-> r.id, cause |-> c] : c \in StopCauses})
    \cup (IF r.phase = "stopping"
          THEN {[m |-> "end", run |-> r.id, frame |-> "none", port |-> p, left |-> l] :
                    p \in IF Len(s.queue) = 0 THEN {"ok"} ELSE Ports, l \in Lefts}
          ELSE {[m |-> "end", run |-> r.id, frame |-> f, port |-> p, left |-> FALSE] :
                    f \in Frames, p \in IF Len(s.queue) = 0 THEN {"ok"} ELSE Ports})

TickMoves(s) ==
    LET stopping == \E r \in s.runs : r.phase = "stopping"
        days == {s.day, s.day + 1} \cap Days
    IN {[m |-> "tick", day |-> d, grace |-> TRUE, port |-> p, left |-> l] :
            d \in days, p \in Ports, l \in IF stopping THEN Lefts ELSE {FALSE}}
       \cup (IF s.dying # {} \/ stopping
             THEN {[m |-> "tick", day |-> d, grace |-> FALSE, port |-> p, left |-> FALSE] :
                      d \in days, p \in Ports}
             ELSE {})

Moves(s) ==
    {[m |-> "start", agent |-> a, port |-> p] : a \in Agents, p \in Ports}
    \cup UNION {RunMoves(s, r) : r \in s.runs}
    \cup TickMoves(s)
    \cup (IF s.life <= MaxRestarts
          THEN {[m |-> "restart", day |-> d, alive |-> A, left |-> l] :
                    d \in {s.day, s.day + 1} \cap Days,
                    A \in {X \in SUBSET {r.id : r \in s.runs} : Cardinality(X) <= MaxAlive},
                    l \in IF s.runs = {} THEN {FALSE} ELSE Lefts}
          ELSE {})
    \cup (IF s.dels < MaxDeletes THEN {[m |-> "delete", agent |-> a] : a \in Deletable \ (s.gone \cup s.lost)} ELSE {})
    \cup {[m |-> "off", agent |-> a] : a \in OffAgents \ s.off}

Acked(g, a) == g.store[a].run # 0 /\ g.store[a].dead /\ g.store[a].sent /\ g.gone[a] = "listed"
HCheck(g, a) == g.unknown[a] # 0 /\ g.unknown[a] \notin g.answered

EnvMoves(g) ==
    {[m |-> "ack", agent |-> a] : a \in {x \in Agents : Acked(g, x)}}
    \cup {[m |-> "check", agent |-> a] : a \in {x \in Agents : HCheck(g, x) /\ g.gone[x] = "listed"}}

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
                 !.holds[a] = IF cause \in FailureClass /\ n >= FailsToPause THEN @ \cup {"failures"} ELSE @]

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

HGone(g, ids, by, left) ==
    [g EXCEPT !.store = [a \in Agents |->
                            IF @[a].run \in ids
                            THEN [@[a] EXCEPT !.dead = TRUE, !.left = left, !.by = IF left THEN by ELSE "none"]
                            ELSE @[a]]]

HAck(g, a) ==
    [g EXCEPT !.store[a] = NoMark,
              !.unknown[a] = IF g.store[a].left THEN g.store[a].run ELSE 0]

RECURSIVE HSettle(_, _)
HSettle(g, a) ==
    IF a > NAgents THEN g ELSE HSettle(IF Acked(g, a) THEN HAck(g, a) ELSE g, a + 1)

HMove(g, mv) ==
    CASE mv.m = "tick" ->
            [g EXCEPT !.day = mv.day,
                      !.live = {IF r.stop /\ g.gone[r.agent] # "listed" THEN [r EXCEPT !.why = "deleted"] ELSE r :
                                   r \in @}]
      [] mv.m = "restart" ->
            [HGone(g, g.alive \ mv.alive, "reap", mv.left)
                EXCEPT !.life = @ + 1, !.day = mv.day,
                       !.wait = [a \in Agents |-> [t \in Askers |-> 0]],
                       !.alive = (@ \cap mv.alive) \cup ({r.id : r \in g.live} \cap mv.alive),
                       !.store = [a \in Agents |-> [@[a] EXCEPT !.sent = FALSE]]]
      [] mv.m = "ack" ->
            IF Acked(g, mv.agent) THEN HAck(g, mv.agent) ELSE g
      [] mv.m = "check" ->
            IF HCheck(g, mv.agent) THEN [g EXCEPT !.answered = @ \cup {g.unknown[mv.agent]}] ELSE g
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
               !.owed = IF PortOf(mv) = "no_store"
                        THEN @ \cup {HOwes([id |-> e.run, agent |-> a, day |-> g.day], "failed")} ELSE @,
               !.h = [g EXCEPT
                        !.count[a][g.day] = @ + 1,
                        !.store[a] = [NoMark EXCEPT !.run = e.run, !.dead = PortOf(mv) = "no_store"],
                        !.unknown[a] = 0,
                        !.heldstarts = IF g.holds[a] # {} \/ HCheck(g, a) THEN @ + 1 ELSE @,
                        !.openstarts = IF g.store[a].run # 0 THEN @ + 1 ELSE @,
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
       THEN [acc EXCEPT !.h = [g EXCEPT !.store[e.agent].dead = TRUE],
                        !.owed = @ \cup {HOwes([r EXCEPT !.day = g.day], "failed")}]
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
      [] e.k = "kill" ->
            LET S == {r \in acc.h.live : r.id = e.run}
                x == HLeft(acc, S, FALSE)
                by == IF S # {} /\ (CHOOSE r \in S : TRUE).why = "lapsed" THEN "lapse" ELSE "kill"
            IN [x EXCEPT !.h = [HGone(@, {e.run} \cap {r.id : r \in S}, by, mv.left)
                                   EXCEPT !.store = [a \in Agents |->
                                       IF @[a].run = e.run /\ S # {} THEN [@[a] EXCEPT !.sent = TRUE] ELSE @[a]]]]
      [] e.k = "reap" ->
            LET S == {r \in acc.h.live : r.life < acc.h.life}
                x == HLeft(acc, S, TRUE)
            IN [x EXCEPT !.h = HGone(@, {r.id : r \in S} \ mv.alive, "reap", mv.left)]
      [] e.k = "end_run" ->
            [acc EXCEPT !.h = [@ EXCEPT
                !.wrongsend = IF e.run \in acc.h.alive \/ \E r \in acc.h.live : r.id = e.run THEN @ + 1 ELSE @,
                !.store = [a \in Agents |-> IF @[a].run = e.run THEN [@[a] EXCEPT !.sent = TRUE] ELSE @[a]]]]
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

HFinal(g, r, mv) ==
    IF mv.frame # "none"
    THEN [g EXCEPT !.store[r.agent] = IF @.run = r.id THEN NoMark ELSE @,
                   !.unknown[r.agent] = IF mv.frame = "effect_unknown" THEN r.id ELSE @]
    ELSE [g EXCEPT !.store[r.agent] =
                      IF @.run = r.id
                      THEN [@ EXCEPT !.dead = TRUE, !.sent = TRUE, !.left = mv.left,
                                     !.by = IF ~mv.left THEN "none"
                                            ELSE IF r.stop /\ r.why = "lapsed" THEN "lapse" ELSE "end"]
                      ELSE @]

HUnsent(g) == Cardinality({a \in Agents : /\ g.store[a].run # 0
                                          /\ g.store[a].dead
                                          /\ ~g.store[a].sent
                                          /\ g.gone[a] = "listed"})

Hist(g, mv, res) ==
    LET g0 == HMove(g, mv)
        ends == IF mv.m = "end" THEN {r \in g0.live : r.id = mv.run} ELSE {}
        g1 == IF ends = {} THEN g0 ELSE HFinal(g0, CHOOSE r \in ends : TRUE, mv)
        owed == {HOwes(r, IF r.stop THEN r.why ELSE IF mv.frame # "none" THEN mv.frame ELSE "interrupted") :
                    r \in {x \in ends : g1.gone[x.agent] = "listed"}}
        acc == HFold([h |-> [g1 EXCEPT !.live = @ \ ends], owed |-> owed, press |-> mv.m = "start", acked |-> {}],
                     res.out, 1, mv)
        g2 == [acc.h EXCEPT !.wrongend = @ + Cardinality(acc.owed),
                            !.forgotten = IF mv.m = "restart" THEN @ + HUnsent(acc.h) ELSE @]
    IN HAfter(g, IF StoreWaits THEN g2 ELSE HSettle(g2, 1), mv)

Init == st = Init0 /\ h = InitH
Next == \E mv \in Moves(st) \cup EnvMoves(h) :
            LET res == Step(st, mv, Facts(h)) IN st' = res.st /\ h' = Hist(h, mv, res)
Now == [st EXCEPT !.f = Facts(h)]
Bound == st.nrun <= MaxRuns
View == <<[st EXCEPT !.ready = {}, !.dying = {}, !.owing = IF StoreWaits THEN @ ELSE {},
                     !.runs = {[r EXCEPT !.phase = IF @ = "stopping" THEN @ ELSE "live"] : r \in @}],
          h>>

LiveRuns(a) == {r \in h.live : r.agent = a}

C1 == /\ Cardinality({r \in h.live : r.agent \in NeedsModel}) <= 1
      /\ Cardinality(h.live) <= MaxLive
C2 == \A r \in h.live : r.life = h.life
C3 == \A a \in Agents :
        Cardinality(LiveRuns(a)) + h.wait[a]["person"] + h.wait[a]["schedule"] <= 1
C4 == h.heldstarts = 0 /\ h.unacked = 0 /\ h.openstarts = 0
C5 == h.alone = 0 /\ h.extra = 0
C6 == /\ h.after = 0
      /\ \A a \in Agents : \A d \in Days : h.sched[a][d] <= 1
C7 == \A a \in Agents : \A d \in Days : h.count[a][d] <= StartsPerDay
C8 == /\ h.gonestarts = 0
      /\ \A r \in h.live : h.gone[r.agent] = "seen" => r.stop /\ ~r.late
C9 == \A r \in h.live : ~r.late
C10 == h.wrongend = 0
C11 == h.noslot = 0
C12 == h.wrongsend = 0 /\ h.forgotten = 0 /\ HUnsent(h) = 0

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
ReachQueueFull == \A a \in Agents : Start(Now, a, "person", "ok").r # "queue_full"
ReachQueueIdle == [][~(/\ st.runs = {}
                       /\ Len(st.queue) > 0
                       /\ st'.life = st.life
                       /\ st'.nrun > st.nrun
                       /\ Len(st'.queue) < Len(st.queue))]_vars
ReachBudget == \A a \in Agents : Decide(Now, a) # "budget"
ReachTwoLive == ~(/\ Cardinality(h.live) = 2
                  /\ Cardinality({r \in h.live : r.agent \in NeedsModel}) = 1)
ReachRestart == [][~(st'.life > st.life /\ st.runs # {} /\ Len(st.queue) > 0)]_vars
ReachRestartDay == [][~(st'.life > st.life /\ st'.day > st.day)]_vars
ReachRestartStopping == [][~(st'.life > st.life /\ \E r \in st.runs : r.phase = "stopping")]_vars
ReachGoneStart == \A a \in Agents : ~(a \in st.gone /\ Press(Now, a, "ok").r = "state")
ReachUnknownAgent == \A a \in Agents : ~(a \in st.lost /\ Press(Now, a, "ok").r = "unknown_agent")
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
                                           /\ Decide([st' EXCEPT !.f = Facts(h')], a) \in {"go", "wait"}
                                           /\ ~SlotDue(st', a))]_vars
AckOf(a) == h.store[a].run # 0 /\ h.store[a].left /\ h'.store[a].run = 0 /\ h'.unknown[a] = h.store[a].run
ReachCheckKill == [][~(\E a \in Agents : AckOf(a) /\ h.store[a].by = "kill")]_vars
ReachCheckLapse == [][~(\E a \in Agents : AckOf(a) /\ h.store[a].by = "lapse")]_vars
ReachCheckRestart == [][~(\E a \in Agents : AckOf(a) /\ h.store[a].by = "reap")]_vars
ReachCheckAnswered == [][~(\E a \in Agents : /\ h.unknown[a] # 0
                                             /\ h.unknown[a] \in h.answered
                                             /\ h'.count[a] # h.count[a])]_vars
ReachStoreOpen == \A a \in Agents : ~(/\ h.store[a].run # 0
                                       /\ Decide(Now, a) = "state"
                                       /\ Decide([Now EXCEPT !.f.open[a] = 0], a) \in {"go", "wait"})
ReachAlive == h.alive = {}
ReachOwedAgain == [][~(st'.life > st.life /\ \E a \in Agents : /\ h.store[a].run # 0
                                                             /\ h.store[a].dead
                                                             /\ h'.store[a].run = h.store[a].run
                                                             /\ h'.store[a].sent)]_vars
ReachSentAgain == [][~(st.owing # {} /\ st'.owing = st.owing /\ st'.life = st.life /\ h'.day # h.day)]_vars
ReachFailedOwed == ~(st.life = 1 /\ st.owing # {})
ReachFailedSentAgain == [][~(st.life = 1 /\ st.owing # {} /\ st'.owing = st.owing /\ h'.day # h.day)]_vars
ReachOwingEnds == [][~(st.owing # {} /\ st'.owing = {} /\ st'.life = st.life)]_vars

StaysLive == \A r \in h.live : h.gone[r.agent] = "listed"
=============================================================================
