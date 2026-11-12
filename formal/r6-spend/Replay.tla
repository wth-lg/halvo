------------------------------- MODULE Replay -------------------------------
EXTENDS Hold, Sequences, FiniteSets, TLC, Committed

RECURSIVE Walk(_, _, _, _, _)
Walk(a, h, moves, i, told) ==
    IF i > Len(moves) THEN told
    ELSE LET line == moves[i]
             res == Step(a, h, [name |-> line.move, arg |-> line.arg])
             same == /\ res.answer = line.answer
                     /\ res.hold.state = line.state
                     /\ res.acct.spent = line.spent
                     /\ res.acct.held = line.held
                     /\ res.hold.cost = line.cost
         IN Walk(res.acct, res.hold, moves, i + 1,
                 [same |-> told.same /\ same,
                  pairs |-> told.pairs \cup {<<h.state, line.move>>},
                  causes |-> told.causes \cup (IF line.answer = "ok" THEN {} ELSE {line.answer})])

Told(trace) ==
    Walk([cap |-> trace.cap, active |-> trace.active = "yes",
          spent |-> trace.spent, held |-> trace.held],
         NewHold, trace.moves, 1, [same |-> TRUE, pairs |-> {}, causes |-> {}])

ASSUME LET all == [i \in DOMAIN Traces |-> Told(Traces[i])]
       IN PrintT("replay count=" \o ToString(Len(Traces))
                 \o " accepted=" \o ToString(Cardinality({i \in DOMAIN Traces : all[i].same}))
                 \o " pairs=" \o ToString(Cardinality(UNION {all[i].pairs : i \in DOMAIN Traces}))
                 \o " causes=" \o ToString(Cardinality(UNION {all[i].causes : i \in DOMAIN Traces})))

=============================================================================
