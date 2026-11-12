------------------------------- MODULE Admit -------------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS Slots, FurtherA, FurtherB, Compiled, ListMax, EditMax, RunsMax, RunsOnBothSteps, Fault

VARIABLES newest, edits, runs, restarted, started, last

vars == <<newest, edits, runs, restarted, started, last>>

K == 2

States == <<"none", "candidate", "provisional", "active", "reverted", "withdrawn", "refused">>

Moves == <<"propose_ok", "propose_refused", "keep", "drop", "use_ok", "use_fail_counted",
           "use_fail_other", "recompile">>

Guard(state, move, clean) ==
    IF state = "provisional" /\ move = "use_ok" /\ clean + 1 < K THEN "clean_below"
    ELSE IF state = "provisional" /\ move = "use_ok" /\ clean + 1 = K THEN "clean_reaches"
    ELSE "-"

EditStep(state, move, clean) ==
    LET to(next, count) == [answer |-> next, state |-> next, clean |-> count]
    IN CASE state = "none" /\ move = "propose_ok" -> to("candidate", clean)
         [] state = "none" /\ move = "propose_refused" -> to("refused", clean)
         [] state = "candidate" /\ move = "keep" -> to("provisional", 0)
         [] state = "candidate" /\ move = "drop" -> to("refused", clean)
         [] state = "provisional" /\ move = "use_ok" /\ clean + 1 < K -> to("provisional", clean + 1)
         [] state = "provisional" /\ move = "use_ok" /\ clean + 1 = K -> to("active", K)
         [] state = "provisional" /\ move = "use_fail_counted" -> to("reverted", clean)
         [] state = "provisional" /\ move = "use_fail_other" -> to("provisional", clean)
         [] state = "active" /\ move = "use_ok" -> to("active", clean)
         [] state = "active" /\ move = "use_fail_counted" ->
                to(IF Fault = "revert_active" THEN "reverted" ELSE "active", clean)
         [] state = "active" /\ move = "use_fail_other" -> to("active", clean)
         [] state \in {"provisional", "active", "reverted"} /\ move = "recompile" ->
                to("withdrawn", clean)
         [] state = "reverted" /\ move = "use_ok" /\ Fault = "reverted_returns" ->
                to("provisional", clean)
         [] OTHER -> [answer |-> "edit", state |-> state, clean |-> clean]

ASSUME \A i \in DOMAIN States, j \in DOMAIN Moves, clean \in 0..K :
          Guard(States[i], Moves[j], clean) = "-" /\ (States[i] # "provisional" \/ Moves[j] # "use_ok")
              => EditStep(States[i], Moves[j], clean) =
                     [EditStep(States[i], Moves[j], 0) EXCEPT !.clean =
                         IF States[i] = "candidate" /\ Moves[j] = "keep" THEN 0 ELSE clean]

ASSUME EditStep("provisional", "use_ok", K).answer = "edit"

Steps == {1, 2}

NoStep == 3

Bases == {1, 2}

OtherKey == <<0, 0>>

KeyOf(base, n) == <<base, n>>

Further(n) == IF n = 1 THEN FurtherA ELSE FurtherB

Range(q) == {q[i] : i \in DOMAIN q}

Latest(S) == CHOOSE i \in S : \A j \in S : j <= i

Lines == {"ok", "other", "counted", "recovered"}

Row(state, clean, users, by, verified) ==
    [state |-> state, clean |-> clean, runs |-> users, by |-> by, verified |-> verified]

Now(edit) == edit.revs[Len(edit.revs)]

At(store, ref) == store[ref[1]][ref[2]][ref[3]]

Refs(store) ==
    UNION {UNION {{<<b, n, i>> : i \in DOMAIN store[b][n]} : n \in Steps} : b \in Bases}

Brought(store, ref) ==
    LET edit == At(store, ref)
    IN IF edit.on = <<>> THEN Range(edit.list)
       ELSE Range(edit.list) \ Range(At(store, edit.on).list)

Barred(store, b, n) ==
    LET gone == {j \in DOMAIN store[b][n] : Now(store[b][n][j]).state = "reverted"}
    IN IF Fault = "bar_newest_revert"
       THEN IF gone = {} THEN {} ELSE Brought(store, <<b, n, Latest(gone)>>)
       ELSE UNION {Brought(store, <<b, n, i>>) : i \in gone}

Later(a, b) == a[1] > b[1] \/ (a[1] = b[1] /\ a[3] >= b[3])

InForce(store, top, n) ==
    LET among(states) ==
            {ref \in Refs(store) :
                 /\ ref[2] = n
                 /\ (ref[1] = top \/ Fault = "serve_lower")
                 /\ Now(At(store, ref)).state \in states}
        last_of(S) == CHOOSE ref \in S : \A other \in S : Later(ref, other)
        provisional == among({"provisional"})
        standing ==
            among(IF Fault = "serve_reverted" THEN {"active", "reverted"} ELSE {"active"})
    IN IF provisional # {} THEN last_of(provisional)
       ELSE IF Fault = "revert_compiled" /\ among({"reverted"}) # {} THEN <<>>
       ELSE IF standing # {} THEN last_of(standing)
       ELSE <<>>

Served(store, ref, top, n) ==
    IF ref = <<>> THEN [key |-> KeyOf(top, n), learned |-> <<>>]
    ELSE [key |-> At(store, ref).key, learned |-> At(store, ref).list]

NoCandidate == [new |-> <<>>, key |-> OtherKey]

Quiet == [counted |-> {}, clean |-> {}, second |-> FALSE]

Idle ==
    [phase |-> "idle", id |-> 0, base |-> 0, served |-> [n \in Steps |-> <<>>],
     step |-> [n \in Steps |-> [key |-> OtherKey, learned |-> <<>>]],
     candidate |-> [n \in Steps |-> NoCandidate]]

Lost == [Idle EXCEPT !.phase = "lost"]

Loaded(s) == runs[s].phase = "loaded"

Holds(run, n) == run.candidate[n].new # <<>>

Held(run, n) ==
    (IF Fault = "compiled_new" THEN {} ELSE Compiled)
        \cup (IF Fault = "held_compiled_only" THEN {} ELSE Range(run.step[n].learned))

Unrepeated(q) == IF Len(q) = 2 /\ q[1] = q[2] /\ Fault # "repeat_kept" THEN <<q[1]>> ELSE q

New(proposal, held) == Unrepeated(SelectSeq(proposal, LAMBDA x : x \notin held))

AdmitCause(free, has_base, same_but_target, new_count, rejected, edits_for_step) ==
    IF free THEN "other"
    ELSE IF ~has_base THEN "base"
    ELSE IF ~same_but_target /\ Fault # "no_scope" THEN "scope"
    ELSE IF new_count = 0 THEN "locator"
    ELSE IF rejected /\ Fault \notin {"no_bar", "no_bar_admit"} THEN "rejected"
    ELSE IF edits_for_step >= EditMax /\ Fault # "no_full" THEN "full"
    ELSE "ok"

Facts(s, n) ==
    IF n \in Steps /\ (runs[s].base = newest \/ Fault = "admit_stale")
    THEN [has_base |-> TRUE, held |-> Held(runs[s], n), barred |-> Barred(edits, newest, n),
          edits_for_step |-> Len(edits[newest][n]), key |-> KeyOf(runs[s].base, n)]
    ELSE [has_base |-> FALSE, held |-> {}, barred |-> {}, edits_for_step |-> 0,
          key |-> KeyOf(runs[s].base, n)]

Answered(facts, proposal, key, free) ==
    LET new == IF facts.has_base THEN New(proposal, facts.held) ELSE <<>>
    IN [cause |-> AdmitCause(free, facts.has_base, key = facts.key, Len(new),
                             Range(new) \cap facts.barred # {}, facts.edits_for_step),
        new |-> new]

Answer(s, n, proposal, key, free) == Answered(Facts(s, n), proposal, key, free)

Proposals(n) ==
    LET locators == IF n \in Steps THEN Compiled \cup Further(n) ELSE Compiled
    IN {<<x>> : x \in locators} \cup {<<x, y>> : x \in locators, y \in locators}

UseMove(counted, verified, ok, any) ==
    IF Fault = "ok_first" /\ verified /\ ok THEN "use_ok"
    ELSE IF counted THEN "use_fail_counted"
    ELSE IF verified /\ ok THEN "use_ok"
    ELSE IF any THEN "use_fail_other"
    ELSE "none"

MoveOf(verified, lines) ==
    UseMove(/\ "counted" \in lines
            /\ Fault # "counted_ignored"
            /\ (verified \/ Fault # "counted_verified"),
            verified \/ Fault = "ok_unverified",
            "ok" \in lines \/ (Fault = "other_as_ok" /\ "other" \in lines),
            lines # {})

Moved(edit, move, id, verified) ==
    LET now == Now(edit)
        step == EditStep(now.state, move, now.clean)
        users == IF move = "use_ok" THEN now.runs \cup {id} ELSE now.runs
        state == IF Fault = "revert_gone" /\ step.state = "reverted" THEN "gone" ELSE step.state
    IN IF move = "none" \/ step.answer = "edit"
              \/ (step.state = now.state /\ step.clean = now.clean)
              \/ (Fault = "never_active" /\ step.state = "active" /\ now.state = "provisional")
       THEN edit
       ELSE [edit EXCEPT !.revs = Append(@, Row(state, step.clean, users, id, verified))]

Withdraw(store, top, id, verified) ==
    [b \in Bases |-> [n \in Steps |->
        IF b < top
        THEN [i \in DOMAIN store[b][n] |->
                 IF Fault = "second_commit_provisional_only" /\ id = 0
                        /\ Now(store[b][n][i]).state # "provisional"
                 THEN store[b][n][i]
                 ELSE Moved(store[b][n][i], "recompile", id, verified)]
        ELSE store[b][n]]]

Use(store, run, n, verified, lines) ==
    LET target == IF Fault = "use_in_force" THEN InForce(store, newest, n) ELSE run.served[n]
        once(edit) == Moved(edit, MoveOf(verified, lines), run.id, verified)
    IN [b \in Bases |-> [m \in Steps |->
           IF m # n THEN store[b][m]
           ELSE [i \in DOMAIN store[b][n] |->
                    IF target # <<b, n, i>> THEN store[b][n][i]
                    ELSE IF Fault = "double_use" THEN once(once(store[b][n][i]))
                    ELSE once(store[b][n][i])]]]

Base(store, b, n) ==
    LET standing ==
            {i \in DOMAIN store[b][n] :
                 Now(store[b][n][i]).state = "active" \/ Fault = "base_any_state"}
    IN IF standing = {} THEN <<>> ELSE <<b, n, Latest(standing)>>

BuildList(new, base) ==
    LET joined == IF Fault = "base_first"
                  THEN base \o SelectSeq(new, LAMBDA x : x \notin Range(base))
                  ELSE new \o SelectSeq(base, LAMBDA x : x \notin Range(new))
    IN IF Len(joined) <= ListMax \/ Fault = "no_cut" THEN joined
       ELSE IF Fault = "cut_front" THEN SubSeq(joined, Len(joined) - ListMax + 1, Len(joined))
       ELSE SubSeq(joined, 1, ListMax)

Left(store, run, n) ==
    LET base == Base(store, run.base, n)
        listed == IF base = <<>> \/ Fault = "left_ignores_base" THEN {}
                  ELSE Range(At(store, base).list)
    IN SelectSeq(run.candidate[n].new, LAMBDA x : x \notin listed)

CandidateMove(store, run, n, verified, recovered) ==
    LET stored == store[run.base][n]
        left == Left(store, run, n)
    IN IF /\ Fault # "never_keep"
          /\ (verified \/ Fault = "keep_unverified")
          /\ (recovered \/ Fault = "keep_unrecovered")
          /\ ((\A i \in DOMAIN stored : Now(stored[i]).state # "provisional")
                  \/ Fault = "no_pending")
          /\ (run.base = newest \/ Fault = "stale_base")
          /\ left # <<>>
          /\ (Range(left) \cap Barred(store, run.base, n) = {}
                  \/ Fault \in {"no_bar", "no_bar_at_finish"})
          /\ (Len(stored) < EditMax \/ Fault = "no_full")
       THEN "keep"
       ELSE "drop"

Stored(store, run, n, verified, recovered) ==
    LET base == Base(store, run.base, n)
        step == EditStep("candidate", "keep", 0)
    IN [list |-> BuildList(Left(store, run, n),
                           IF base = <<>> THEN <<>> ELSE At(store, base).list),
        on |-> base,
        key |-> run.candidate[n].key,
        revs |-> <<Row(step.state, step.clean, {run.id}, run.id, verified)>>,
        recovered |-> recovered,
        onWas |-> IF base = <<>> THEN "none" ELSE Now(At(store, base)).state]

Keep(store, run, n, verified, recovered) ==
    [b \in Bases |->
        IF /\ b = run.base
           /\ Holds(run, n)
           /\ EditStep("candidate", CandidateMove(store, run, n, verified, recovered), 0).state
                  = "provisional"
        THEN Append(store[b][n], Stored(store, run, n, verified, recovered))
        ELSE store[b][n]]

LogsOf(run, n) ==
    {used \cup kept :
         used \in (IF run.served[n] = <<>> /\ Fault # "use_in_force" THEN {{}}
                   ELSE SUBSET (Lines \ {"recovered"})),
         kept \in (IF Holds(run, n) THEN {{}, {"recovered"}} ELSE {{}})}

Finished(run, verified, log) ==
    LET top == IF Fault = "withdraw_below_loaded" THEN run.base
               ELSE IF Fault = "withdraw_newest_too" THEN newest + 1
               ELSE newest
        lowered == IF Fault \in {"withdraw_last", "no_finish_withdraw"} THEN edits
                   ELSE Withdraw(edits, top, run.id, verified)
        lines(n) == IF Fault = "counted_of_either_step" /\ \E m \in Steps : "counted" \in log[m]
                    THEN log[n] \cup {"counted"}
                    ELSE log[n]
        end(n) == Keep(Use(lowered, run, n, verified, lines(n)), run, n, verified,
                       "recovered" \in log[n])
        moved == [b \in Bases |-> [n \in Steps |-> end(n)[b]]]
    IN IF Fault = "withdraw_last" THEN Withdraw(moved, newest, run.id, verified) ELSE moved

Ended(run) ==
    {[verified |-> verified, log |-> <<first, second>>] :
         verified \in BOOLEAN, first \in LogsOf(run, 1), second \in LogsOf(run, 2)}

Seen(end) ==
    [counted |-> {n \in Steps : "counted" \in end.log[n]},
     clean |-> {n \in Steps : /\ end.verified
                                /\ "ok" \in end.log[n]
                                /\ "counted" \notin end.log[n]},
     second |-> FALSE]

Lose(table) ==
    [s \in Slots |->
        IF table[s].phase # "loaded" THEN table[s]
        ELSE IF Fault = "lost_finish_moves"
        THEN [table[s] EXCEPT !.phase = "lost", !.candidate = [n \in Steps |-> NoCandidate]]
        ELSE Lost]

Init ==
    /\ newest = 1
    /\ edits = [b \in Bases |-> [n \in Steps |-> <<>>]]
    /\ runs = [s \in Slots |-> Idle]
    /\ restarted = FALSE
    /\ started = 0
    /\ last = Quiet

Load(s) ==
    LET top == IF Fault = "stale_top" THEN 1 ELSE newest
    IN /\ runs[s].phase = "idle"
       /\ started < RunsMax
       /\ runs' = [runs EXCEPT ![s] =
                      [phase |-> "loaded", id |-> started + 1, base |-> newest,
                       served |-> [n \in Steps |-> InForce(edits, top, n)],
                       step |-> [n \in Steps |->
                                    Served(edits, InForce(edits, top, n), newest, n)],
                       candidate |-> [n \in Steps |-> NoCandidate]]]
       /\ started' = started + 1
       /\ last' = Quiet
       /\ UNCHANGED <<newest, edits, restarted>>

Admitted(s, n) ==
    LET facts == Facts(s, n)
        asked == Proposals(n) \X {facts.key, OtherKey} \X BOOLEAN
        answer(t) == Answered(facts, t[1], t[2], t[3])
        move(t) == IF answer(t).cause = "ok" THEN "propose_ok" ELSE "propose_refused"
        taken == {t \in asked : EditStep("none", move(t), 0).state = "candidate"}
    IN {[new |-> answer(t).new, key |-> t[2]] : t \in taken}

Propose(s, n) ==
    /\ Loaded(s)
    /\ \E candidate \in Admitted(s, n) : runs' = [runs EXCEPT ![s].candidate[n] = candidate]
    /\ last' = Quiet
    /\ UNCHANGED <<newest, edits, restarted, started>>

Finish(s) ==
    /\ Loaded(s)
    /\ \E end \in Ended(runs[s]) :
           /\ edits' = Finished(runs[s], end.verified, end.log)
           /\ last' = Seen(end)
    /\ runs' = [runs EXCEPT ![s] = Idle]
    /\ UNCHANGED <<newest, restarted, started>>

FinishLost(s) ==
    /\ runs[s].phase = "lost"
    /\ IF Fault = "lost_finish_moves"
       THEN \E end \in Ended(runs[s]) : edits' = Finished(runs[s], end.verified, end.log)
       ELSE IF Fault = "lost_finish_withdraws"
       THEN edits' = Withdraw(edits, newest, 0, FALSE)
       ELSE edits' = edits
    /\ runs' = [runs EXCEPT ![s] = Idle]
    /\ last' = Quiet
    /\ UNCHANGED <<newest, restarted, started>>

Restart ==
    /\ ~restarted
    /\ \E s \in Slots : Loaded(s)
    /\ restarted' = TRUE
    /\ runs' = IF Fault = "keep_across_restart" THEN runs ELSE Lose(runs)
    /\ last' = Quiet
    /\ UNCHANGED <<newest, edits, started>>

Compile(stop, lose) ==
    /\ newest = 1
    /\ newest' = 2
    /\ edits' = IF stop THEN edits ELSE Withdraw(edits, 2, 0, FALSE)
    /\ runs' = IF lose THEN Lose(runs) ELSE runs
    /\ last' = [Quiet EXCEPT !.second = ~stop]
    /\ UNCHANGED <<restarted, started>>

Next ==
    \/ \E s \in Slots : Load(s)
    \/ \E s \in Slots, n \in Steps \cup {NoStep} : Propose(s, n)
    \/ \E s \in Slots : Finish(s)
    \/ \E s \in Slots : FinishLost(s)
    \/ Restart
    \/ \E stop \in BOOLEAN, lose \in BOOLEAN : (lose => stop) /\ Compile(stop, lose)

StepIdle(n) ==
    /\ \A b \in Bases : edits[b][n] = <<>>
    /\ \A s \in Slots : ~Holds(runs[s], n)

Bound == started <= RunsOnBothSteps \/ StepIdle(2)

Symmetry ==
    {a @@ b @@ c : a \in Permutations(FurtherA), b \in Permutations(FurtherB),
                   c \in Permutations(Slots)}

Loads(s) == runs[s].phase = "idle" /\ runs'[s].phase = "loaded"

Ends(s) == runs[s].phase = "loaded" /\ runs'[s].phase = "idle"

EndsLost(s) == runs[s].phase = "lost" /\ runs'[s].phase = "idle"

Added(ref) ==
    SubSeq(At(edits', ref).revs, Len(At(edits, ref).revs) + 1, Len(At(edits', ref).revs))

Due(n) ==
    LET of(state) ==
            {i \in DOMAIN edits[newest][n] : Now(edits[newest][n][i]).state = state}
    IN IF of("provisional") # {} THEN <<newest, n, Latest(of("provisional"))>>
       ELSE IF of("active") # {} THEN <<newest, n, Latest(of("active"))>>
       ELSE <<>>

Bars(store, b, n) ==
    UNION {Brought(store, <<b, n, i>>) :
               i \in {j \in DOMAIN store[b][n] :
                          \E k \in DOMAIN store[b][n][j].revs :
                              store[b][n][j].revs[k].state = "reverted"}}

Q1 ==
    [][\A s \in Slots : Loads(s) => \A n \in Steps :
           LET ref == runs'[s].served[n]
           IN /\ ref # <<>> =>
                      /\ ref \in Refs(edits)
                      /\ Now(At(edits, ref)).state \in {"provisional", "active"}
                      /\ ref[1] = newest
              /\ ref = Due(n)]_vars

Q2 ==
    [](\A ref \in Refs(edits) :
           LET edit == At(edits, ref)
           IN /\ \A i \in DOMAIN edit.revs : edit.revs[i].runs # {}
              /\ edit.revs[1].by \in edit.revs[1].runs
              /\ edit.revs[1].verified
              /\ edit.recovered)

Q3 == [][\A s \in Slots : Loads(s) => \A n \in Steps :
             runs'[s].step[n].key = KeyOf(newest, n)]_vars

Q4 ==
    [](\A ref \in Refs(edits) :
           LET edit == At(edits, ref)
           IN edit.on # <<>> =>
                  /\ edit.onWas = "active"
                  /\ edit.on[1] = ref[1]
                  /\ edit.on[2] = ref[2])

Q5 ==
    [][\A s \in Slots : Loads(s) => \A n \in Steps :
           Range(runs'[s].step[n].learned) \cap (Bars(edits, newest, n) \cup Compiled) = {}]_vars

Q6 ==
    [](\A ref \in Refs(edits) :
           LET revs == At(edits, ref).revs
           IN \A i \in DOMAIN revs :
                  /\ revs[i].state = "active" =>
                         /\ i > 1
                         /\ revs[i - 1].state = "provisional"
                         /\ revs[i].clean = K
                         /\ Cardinality({j \in 2..i : revs[j].runs # revs[j - 1].runs}) = K
                  /\ revs[i].state = "reverted" =>
                         /\ i > 1
                         /\ revs[i - 1].state = "provisional")

Q7 ==
    [](/\ \A ref \in Refs(edits) :
              LET edit == At(edits, ref)
              IN /\ Len(edit.list) \in 1..ListMax
                 /\ Cardinality(Range(edit.list)) = Len(edit.list)
       /\ \A n \in Steps :
              Cardinality({i \in DOMAIN edits[newest][n] :
                               Now(edits[newest][n][i]).state = "provisional"}) <= 1)

Q7Step == [][\A ref \in Refs(edits') \ Refs(edits) : ref[1] = newest]_vars

Q8 ==
    [][\A s \in Slots : Ends(s) => \A n \in Steps :
           LET ref == runs[s].served[n]
           IN (/\ ref # <<>>
               /\ n \in last'.counted
               /\ ref[1] = newest
               /\ Now(At(edits, ref)).state = "provisional")
                  => Now(At(edits', ref)).state = "reverted"]_vars

Q9 ==
    [][/\ edits' # edits =>
              \A ref \in Refs(edits) :
                  /\ ref[1] < newest' =>
                         \A i \in DOMAIN Added(ref) : Added(ref)[i].state = "withdrawn"
                  /\ ref[1] = newest' =>
                         \A i \in DOMAIN Added(ref) : Added(ref)[i].state # "withdrawn"
       /\ (last'.second \/ \E s \in Slots : Ends(s)) =>
              \A ref \in Refs(edits') :
                  ref[1] < newest' => Now(At(edits', ref)).state = "withdrawn"]_vars

Q10 ==
    [][edits' # edits =>
           /\ \A ref \in Refs(edits) :
                  /\ ref \in Refs(edits')
                  /\ SubSeq(At(edits', ref).revs, 1, Len(At(edits, ref).revs))
                         = At(edits, ref).revs
                  /\ Now(At(edits, ref)).state = "reverted" =>
                         \A i \in DOMAIN Added(ref) : Added(ref)[i].state = "withdrawn"
           /\ newest' = newest =>
                \A n \in Steps : Bars(edits, newest, n) \subseteq Bars(edits', newest, n)]_vars

Q11 ==
    [](/\ \A ref \in Refs(edits) : At(edits, ref).list[1] \in Brought(edits, ref)
       /\ \A b \in Bases, n \in Steps : Len(edits[b][n]) <= EditMax)

Q11Step ==
    [][\A s \in Slots, n \in Steps :
           (/\ runs'[s].phase = "loaded"
            /\ Holds(runs'[s], n)
            /\ runs'[s].candidate[n] # runs[s].candidate[n])
               => LET new == Range(runs'[s].candidate[n].new)
                  IN /\ runs[s].base = newest
                     /\ new \cap Bars(edits, newest, n) = {}
                     /\ new \cap (Range(runs[s].step[n].learned) \cup Compiled) = {}]_vars

Q12 ==
    [](\A ref \in Refs(edits) :
           LET revs == At(edits, ref).revs
           IN \A i \in DOMAIN revs :
                  (revs[i].state = "active" /\ (i = 1 \/ revs[i - 1].state # "active")) =>
                      LET uses == {j \in 2..i : revs[j].runs # revs[j - 1].runs}
                      IN /\ Cardinality(uses) = K
                         /\ Cardinality({revs[j].by : j \in uses}) = K
                         /\ \A j \in uses :
                                /\ revs[j].verified
                                /\ revs[j].by \notin revs[j - 1].runs
                                /\ revs[j].runs = revs[j - 1].runs \cup {revs[j].by})

Q12Step ==
    [][edits' # edits =>
           \A ref \in Refs(edits) : \A i \in DOMAIN Added(ref) :
               LET row == Added(ref)[i]
                   before == IF i = 1 THEN Now(At(edits, ref)) ELSE Added(ref)[i - 1]
               IN row.runs # before.runs =>
                      /\ Len(Added(ref)) = 1
                      /\ ref[2] \in last'.clean
                      /\ \E s \in Slots : /\ Ends(s)
                                          /\ runs[s].served[ref[2]] = ref
                                          /\ runs[s].id = row.by]_vars

Q12Move ==
    [][\A s \in Slots : Ends(s) => \A n \in Steps :
           LET ref == runs[s].served[n]
           IN (/\ ref # <<>>
               /\ n \in last'.clean
               /\ ref[1] = newest
               /\ Now(At(edits, ref)).state = "provisional")
                  => LET before == Now(At(edits, ref))
                         after == Now(At(edits', ref))
                     IN /\ after.clean = before.clean + 1
                        /\ after.runs = before.runs \cup {runs[s].id}
                        /\ after.state = IF after.clean = K THEN "active" ELSE "provisional"]_vars

Q13 ==
    [][/\ edits' # edits =>
              \A ref \in Refs(edits) : \A i \in DOMAIN Added(ref) :
                  Added(ref)[i].state # "withdrawn" =>
                      \E s \in Slots : Ends(s) /\ runs[s].served[ref[2]] = ref
       /\ \A s \in Slots : Loads(s) =>
              /\ runs'[s].base = newest
              /\ \A n \in Steps :
                     runs'[s].served[n] # <<>> => runs'[s].served[n][1] = runs'[s].base
       /\ \A s \in Slots : EndsLost(s) => edits' = edits
       /\ (restarted' /\ ~restarted) => \A s \in Slots : runs'[s].phase # "loaded"]_vars

Q14 ==
    [][edits' # edits =>
           \A ref \in Refs(edits) : \A i \in DOMAIN Added(ref) :
               Added(ref)[i].state = "reverted" => ref[2] \in last'.counted]_vars

=============================================================================
