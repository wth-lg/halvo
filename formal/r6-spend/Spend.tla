------------------------------- MODULE Spend -------------------------------
EXTENDS Hold, Sequences, FiniteSets, TLC

CONSTANTS Runs, MaxRequests, MaxAdmissions, MaxCapChanges, MaxOtherSpends,
          MaxLostAnswers, MaxStops, Hurry, TraceMoves, Fault

Requests == 1..MaxRequests
Estimates == {1, 2}
Amounts == {1, 2}
Caps == 0..3

Breaks == {"grant_without_ruling", "reserve_to_sent", "dispatch_twice",
           "reserve_over_cap", "resend_new_id", "admit_alone", "send_in_flight",
           "send_after_over_cap", "resend_other_amount", "send_when_off",
           "third_send_same_id"}
WrongDesigns == {"grant_kept", "third_send_new_id", "dispatch_on_repeat",
                 "list_until_first_send", "next_request_after_loss",
                 "resend_after_over_cap", "stop_drops_flight", "expired_given_back",
                 "readmit_on_report", "next_after_fail", "self_admission",
                 "room_without_held", "shared_id"}

ASSUME Fault \in {"none"} \cup Breaks \cup WrongDesigns
ASSUME Hurry \in BOOLEAN

VARIABLES ruling, acct, holds, budget, run, gate, coord, saw, last, tape, draw

vars == <<ruling, acct, holds, budget, run, gate, coord, saw, last, tape, draw>>

RunSymmetry == Permutations(Runs)

MaxSends == IF Fault \in {"third_send_new_id", "third_send_same_id"} THEN 3 ELSE 2

Ids ==
    IF Fault = "shared_id" THEN {<<k>> : k \in Requests}
    ELSE Runs \X Requests \X (IF Fault \in {"resend_new_id", "third_send_new_id"}
                              THEN {1, 2} ELSE {1})

IdOf(r, k, n) ==
    IF Fault = "shared_id" THEN <<k>>
    ELSE IF Fault = "resend_new_id" THEN <<r, k, n>>
    ELSE IF Fault = "third_send_new_id" THEN <<r, k, IF n = 3 THEN 2 ELSE 1>>
    ELSE <<r, k, 1>>

Carries(r) == IdOf(r, run[r].made, IF run[r].sends = 0 THEN 1 ELSE run[r].sends)

RECURSIVE SumOver(_, _)
SumOver(f, set) ==
    IF set = {} THEN 0
    ELSE LET x == CHOOSE y \in set : TRUE IN f[x] + SumOver(f, set \ {x})

Blank == [kind |-> "none", sum |-> 0, bound |-> 0]

Ledger(a, h, m) ==
    IF Fault = "reserve_to_sent" /\ m.name = "reserve" /\ Step(a, h, m).answer = "ok"
    THEN [Step(a, h, m) EXCEPT !.hold.state = "sent"]
    ELSE IF Fault = "dispatch_twice" /\ m.name = "dispatch" /\ h.state = "sent"
    THEN Passed(a, h)
    ELSE IF Fault = "reserve_over_cap" /\ m.name = "reserve" /\ h.state = "new"
    THEN Passed([a EXCEPT !.held = @ + m.arg], [h EXCEPT !.state = "held", !.amount = m.arg])
    ELSE Step(a, h, m)

Under(cap) == [acct EXCEPT !.cap = cap]

InForce(cap) == cap = acct.cap \/ budget.caps > 0

Charged(cap) == [budget EXCEPT !.caps = IF cap = acct.cap THEN @ ELSE @ - 1]

Grant(r) ==
    \/ ruling = "hosted"
    \/ Fault = "grant_without_ruling"
    \/ Fault = "grant_kept" /\ run[r].made > 0

SawSend(s, r, k, id, amount) ==
    [s EXCEPT !.ids[r][k] = @ \cup {id},
              !.amounts[r][k] = @ \cup {amount},
              !.sends[r][k] = @ + 1,
              !.unadmitted[r][k] = @ \/ s.live[r] = 0,
              !.out[r] = TRUE,
              !.trail[r] = IF @ = "refused" THEN "sent_refused" ELSE @]

SawLedger(s, id, res) ==
    IF res.answer # "ok" \/ res.hold.state = holds[id].state THEN s
    ELSE [s EXCEPT !.path[id] = Append(@, res.hold.state),
                   !.under = IF holds[id].state = "new" THEN @ \cup {ruling} ELSE @]

SawCall(s, r, k) == [s EXCEPT !.calls[r][k] = @ + 1]

SawQuiet(s, r) ==
    [s EXCEPT !.out[r] = FALSE,
              !.live[r] = IF s.done[r] THEN 0 ELSE @,
              !.done[r] = FALSE]

SawAnswer(s, r, answer) ==
    SawQuiet([s EXCEPT !.trail[r] = IF answer = "over_cap" /\ @ = "clear"
                                    THEN "refused" ELSE @], r)

SawEnd(s, r) ==
    [s EXCEPT !.live[r] = IF s.out[r] THEN @ ELSE 0,
              !.done[r] = s.out[r]]

Off(rec) == [rec EXCEPT !.on = FALSE, !.sends = 0, !.amount = 0, !.stop = FALSE]

Reads(rec, answer) ==
    LET goes == /\ rec.made < MaxRequests
                /\ \/ answer = "ok" /\ ~rec.stop
                   \/ answer = "over_cap" /\ Fault = "send_after_over_cap"
                   \/ answer = "fail" /\ Fault = "next_after_fail"
    IN [rec EXCEPT !.sends = 0,
                   !.amount = 0,
                   !.on = rec.on /\ goes,
                   !.stop = rec.stop /\ goes,
                   !.waits = answer = "over_cap" /\ ~goes]

Retries(rec, answer) ==
    /\ Fault = "resend_after_over_cap"
    /\ answer = "over_cap"
    /\ rec.sends < MaxSends

Ends(r, answer, rn, s, b) ==
    IF rn[r].sends = 0
    THEN /\ gate' = [gate EXCEPT ![r] = "idle"]
         /\ run' = rn
         /\ coord' = coord
         /\ saw' = SawQuiet(s, r)
         /\ budget' = b
    ELSE \/ /\ ~Retries(rn[r], answer)
            /\ gate' = [gate EXCEPT ![r] = "idle"]
            /\ run' = [rn EXCEPT ![r] = Reads(@, answer)]
            /\ coord' = IF Reads(rn[r], answer).on THEN coord
                        ELSE [coord EXCEPT !.list[r] = 0]
            /\ saw' = IF Reads(rn[r], answer).on THEN SawAnswer(s, r, answer)
                      ELSE SawEnd(SawAnswer(s, r, answer), r)
            /\ budget' = b
         \/ /\ Retries(rn[r], answer)
            /\ gate' = [gate EXCEPT ![r] = "lost"]
            /\ run' = rn
            /\ coord' = coord
            /\ saw' = SawAnswer(s, r, answer)
            /\ budget' = b
         \/ /\ Cardinality({q \in Runs : gate[q] = "lost" \/ rn[q].sends >= 2}) < MaxLostAnswers
            /\ budget' = b
            /\ gate' = [gate EXCEPT ![r] = "lost"]
            /\ run' = rn
            /\ coord' = coord
            /\ saw' = SawQuiet(s, r)

Sent(r, k, n, amount, cap) ==
    LET id == IdOf(r, k, n)
        res == Ledger(Under(cap), holds[id], ToReserve(amount))
        rn == [run EXCEPT ![r].made = k, ![r].sends = n, ![r].amount = amount]
        heard == SawLedger(SawSend(saw, r, k, id, amount), id, res)
        early == res.answer = "repeat" /\ Fault = "dispatch_on_repeat"
    IN /\ acct' = res.acct
       /\ holds' = [holds EXCEPT ![id] = res.hold]
       /\ last' = IF res.answer = "ok"
                  THEN [kind |-> "reserve",
                        sum |-> res.acct.spent + res.acct.held,
                        bound |-> res.acct.cap]
                  ELSE Blank
       /\ IF res.answer \in {"ok", "repeat"}
          THEN /\ gate' = [gate EXCEPT ![r] = IF early THEN "upstream" ELSE "reserved"]
               /\ run' = rn
               /\ budget' = Charged(cap)
               /\ coord' = IF Fault = "list_until_first_send"
                           THEN [coord EXCEPT !.list[r] = 0] ELSE coord
               /\ saw' = IF early THEN SawCall(heard, r, k) ELSE heard
          ELSE Ends(r, IF res.answer = "over_cap" THEN "over_cap" ELSE "fail", rn, heard,
                    Charged(cap))

Init ==
    /\ ruling \in {"none", "hosted"}
    /\ acct \in [cap : Caps, active : {TRUE}, spent : {0}, held : {0}]
    /\ holds = [id \in Ids |-> NewHold]
    /\ budget = [caps |-> MaxCapChanges, other |-> MaxOtherSpends, stops |-> MaxStops]
    /\ run = [r \in Runs |-> [on |-> FALSE, waits |-> FALSE, admits |-> 0, made |-> 0,
                              sends |-> 0, amount |-> 0, stop |-> FALSE]]
    /\ gate = [r \in Runs |-> "idle"]
    /\ coord = [room |-> Null, list |-> [r \in Runs |-> 0]]
    /\ saw = [under |-> {},
              path |-> [id \in Ids |-> <<>>],
              ids |-> [r \in Runs |-> [k \in Requests |-> {}]],
              amounts |-> [r \in Runs |-> [k \in Requests |-> {}]],
              sends |-> [r \in Runs |-> [k \in Requests |-> 0]],
              unadmitted |-> [r \in Runs |-> [k \in Requests |-> FALSE]],
              calls |-> [r \in Runs |-> [k \in Requests |-> 0]],
              out |-> [r \in Runs |-> FALSE],
              trail |-> [r \in Runs |-> "clear"],
              room |-> Null,
              live |-> [r \in Runs |-> 0],
              done |-> [r \in Runs |-> FALSE]]
    /\ last = Blank
    /\ tape = ""
    /\ draw = [moves |-> 0, arg |-> 0]

RoomAsRead(a) ==
    IF Fault = "room_without_held"
    THEN (IF a.cap > a.spent THEN a.cap - a.spent ELSE 0)
    ELSE Left(a)

ReadRoom(cap) ==
    /\ InForce(cap)
    /\ coord.room # RoomAsRead(Under(cap)) \/ saw.room # Left(Under(cap))
    /\ acct' = Under(cap)
    /\ budget' = Charged(cap)
    /\ coord' = [coord EXCEPT !.room = RoomAsRead(Under(cap))]
    /\ saw' = [saw EXCEPT !.room = Left(Under(cap))]
    /\ last' = Blank
    /\ UNCHANGED <<ruling, holds, run, gate, tape, draw>>

Listed(r) == IF Fault = "admit_alone" THEN 0 ELSE SumOver(coord.list, Runs \ {r})

RoomUsed ==
    IF Fault = "expired_given_back"
    THEN coord.room + SumOver([id \in Ids |-> IF holds[id].state = "expired"
                                              THEN holds[id].amount ELSE 0], Ids)
    ELSE coord.room

OpenHolds(r) ==
    {id \in UNION {saw.ids[r][k] : k \in Requests} : holds[id].state \in {"held", "sent"}}

Admit(r, estimate) ==
    /\ ~run[r].on
    /\ run[r].admits < MaxAdmissions
    /\ run[r].made < MaxRequests
    /\ gate[r] = "idle"
    /\ coord.room # Null
    /\ Fault = "readmit_on_report" \/ OpenHolds(r) = {}
    /\ estimate + Listed(r) <= RoomUsed
    /\ run' = [run EXCEPT ![r].on = TRUE, ![r].waits = FALSE, ![r].admits = @ + 1]
    /\ coord' = [coord EXCEPT !.list[r] = estimate]
    /\ saw' = [saw EXCEPT !.live[r] = estimate, !.trail[r] = "clear"]
    /\ last' = [kind |-> "admission",
                sum |-> estimate + SumOver(saw.live, Runs \ {r}),
                bound |-> saw.room]
    /\ UNCHANGED <<ruling, acct, holds, budget, gate, tape, draw>>

Stop(r) ==
    /\ run[r].on
    /\ ~run[r].stop
    /\ budget.stops > 0
    /\ budget' = [budget EXCEPT !.stops = @ - 1]
    /\ IF run[r].sends = 0 \/ Fault = "stop_drops_flight"
       THEN /\ run' = [run EXCEPT ![r] = Off(@)]
            /\ gate' = [gate EXCEPT ![r] = IF @ = "lost" THEN "idle" ELSE @]
            /\ coord' = [coord EXCEPT !.list[r] = 0]
            /\ saw' = SawEnd(saw, r)
       ELSE /\ run' = [run EXCEPT ![r].stop = TRUE]
            /\ UNCHANGED <<gate, coord, saw>>
    /\ last' = Blank
    /\ UNCHANGED <<ruling, acct, holds, tape, draw>>

Send(r, amount, cap) ==
    /\ InForce(cap)
    /\ run[r].on \/ Fault = "send_when_off"
    /\ run[r].made < MaxRequests
    /\ (run[r].sends = 0 /\ gate[r] = "idle") \/ Fault = "send_in_flight"
    /\ ~run[r].stop
    /\ Grant(r)
    /\ Sent(r, run[r].made + 1, 1, amount, cap)
    /\ UNCHANGED <<ruling, tape, draw>>

Resend(r, cap) ==
    /\ InForce(cap)
    /\ gate[r] = "lost"
    /\ run[r].sends >= 1
    /\ run[r].sends < MaxSends
    /\ Fault # "next_request_after_loss"
    /\ Grant(r)
    /\ Sent(r, run[r].made, run[r].sends + 1,
            IF Fault = "resend_other_amount" THEN 3 - run[r].amount ELSE run[r].amount, cap)
    /\ UNCHANGED <<ruling, tape, draw>>

GiveUp(r) ==
    /\ gate[r] = "lost"
    /\ run[r].sends >= MaxSends \/ run[r].stop
    /\ gate' = [gate EXCEPT ![r] = "idle"]
    /\ run' = [run EXCEPT ![r] = Off(@)]
    /\ coord' = [coord EXCEPT !.list[r] = 0]
    /\ saw' = SawEnd(saw, r)
    /\ last' = Blank
    /\ UNCHANGED <<ruling, acct, holds, budget, tape, draw>>

Dispatches(r) ==
    /\ gate[r] = "reserved"
    /\ LET id == Carries(r)
           res == Ledger(acct, holds[id], ToDispatch)
       IN IF res.answer = "ok"
          THEN /\ acct' = res.acct
               /\ holds' = [holds EXCEPT ![id] = res.hold]
               /\ gate' = [gate EXCEPT ![r] = "upstream"]
               /\ saw' = SawCall(SawLedger(saw, id, res), r, run[r].made)
               /\ UNCHANGED <<run, coord, budget>>
          ELSE /\ UNCHANGED <<acct, holds>>
               /\ Ends(r, "fail", run, saw, budget)
    /\ last' = Blank
    /\ UNCHANGED <<ruling, tape, draw>>

Voids(r) ==
    /\ gate[r] = "reserved"
    /\ LET id == Carries(r)
           res == Ledger(acct, holds[id], ToVoid)
       IN /\ acct' = res.acct
          /\ holds' = [holds EXCEPT ![id] = res.hold]
          /\ Ends(r, "fail", run, SawLedger(saw, id, res), budget)
    /\ last' = Blank
    /\ UNCHANGED <<ruling, tape, draw>>

Settles(r, cost) ==
    /\ gate[r] = "upstream"
    /\ cost <= holds[Carries(r)].amount
    /\ LET id == Carries(r)
           res == Ledger(acct, holds[id], ToSettle(cost))
       IN /\ acct' = res.acct
          /\ holds' = [holds EXCEPT ![id] = res.hold]
          /\ Ends(r, IF res.answer = "ok" THEN "ok" ELSE "fail", run, SawLedger(saw, id, res),
                  budget)
    /\ last' = Blank
    /\ UNCHANGED <<ruling, tape, draw>>

Cuts(r) ==
    /\ gate[r] = "upstream"
    /\ Ends(r, "fail", run, saw, budget)
    /\ last' = Blank
    /\ UNCHANGED <<ruling, acct, holds, tape, draw>>

Works(r, id) == gate[r] \in {"reserved", "upstream"} /\ Carries(r) = id

Expires(id) ==
    /\ holds[id].state \in {"held", "sent"}
    /\ LET res == Ledger(acct, holds[id], ToExpire)
       IN /\ acct' = res.acct
          /\ holds' = [holds EXCEPT ![id] = res.hold]
          /\ IF \E r \in Runs : Works(r, id)
             THEN \E r \in Runs : Works(r, id) /\ Ends(r, "fail", run, SawLedger(saw, id, res),
                                                     budget)
             ELSE /\ saw' = SawLedger(saw, id, res)
                  /\ UNCHANGED <<budget, run, gate, coord>>
    /\ last' = Blank
    /\ UNCHANGED <<ruling, tape, draw>>

Ended(r) ==
    /\ Fault # "shared_id"
    /\ run[r].made > 0
    /\ run[r].sends = 0
    /\ gate[r] = "idle"
    /\ saw.ids[r][run[r].made] # {}
    /\ \A id \in saw.ids[r][run[r].made] : holds[id].state \notin {"held", "sent"}

Forget(r) ==
    /\ Ended(r)
    /\ LET gone == saw.ids[r][run[r].made]
           over == run[r].made = MaxRequests /\ ~run[r].on
       IN /\ holds' = [id \in Ids |-> IF id \in gone THEN NewHold ELSE holds[id]]
          /\ saw' = [saw EXCEPT !.path = [id \in Ids |-> IF id \in gone THEN <<>> ELSE @[id]],
                                !.ids[r][run[r].made] = {},
                                !.amounts[r][run[r].made] = {},
                                !.sends[r][run[r].made] = 0,
                                !.unadmitted[r][run[r].made] = FALSE,
                                !.calls[r][run[r].made] = 0,
                                !.trail[r] = IF over THEN "clear" ELSE @]
          /\ run' = IF over
                    THEN [run EXCEPT ![r].admits = MaxAdmissions, ![r].waits = FALSE]
                    ELSE run
    /\ last' = Blank
    /\ UNCHANGED <<ruling, acct, budget, gate, coord, tape, draw>>

OtherSpends(cap) ==
    /\ InForce(cap)
    /\ budget.other > 0
    /\ Left(Under(cap)) >= 1
    /\ acct' = [Under(cap) EXCEPT !.spent = @ + 1]
    /\ budget' = [Charged(cap) EXCEPT !.other = @ - 1]
    /\ last' = Blank
    /\ UNCHANGED <<ruling, holds, run, gate, coord, saw, tape, draw>>

Skip(r) ==
    /\ Fault = "next_request_after_loss"
    /\ gate[r] = "lost"
    /\ gate' = [gate EXCEPT ![r] = "idle"]
    /\ IF run[r].made < MaxRequests /\ ~run[r].stop
       THEN /\ run' = [run EXCEPT ![r].sends = 0, ![r].amount = 0]
            /\ UNCHANGED <<coord, saw>>
       ELSE /\ run' = [run EXCEPT ![r] = Off(@)]
            /\ coord' = [coord EXCEPT !.list[r] = 0]
            /\ saw' = SawEnd(saw, r)
    /\ last' = Blank
    /\ UNCHANGED <<ruling, acct, holds, budget, tape, draw>>

Resume(r) ==
    /\ Fault = "self_admission"
    /\ run[r].waits
    /\ run[r].made < MaxRequests
    /\ run' = [run EXCEPT ![r].on = TRUE, ![r].waits = FALSE]
    /\ last' = Blank
    /\ UNCHANGED <<ruling, acct, holds, budget, gate, coord, saw, tape, draw>>

Withdraw ==
    /\ Fault = "grant_kept"
    /\ ruling = "hosted"
    /\ ruling' = "none"
    /\ last' = Blank
    /\ UNCHANGED <<acct, holds, budget, run, gate, coord, saw, tape, draw>>

Moves ==
    \/ \E cap \in Caps : ReadRoom(cap) \/ OtherSpends(cap)
    \/ \E r \in Runs, estimate \in Estimates : Admit(r, estimate)
    \/ \E r \in Runs, amount \in Amounts, cap \in Caps : Send(r, amount, cap)
    \/ \E r \in Runs, cost \in Amounts : Settles(r, cost)
    \/ \E r \in Runs, cap \in Caps : Resend(r, cap)
    \/ \E r \in Runs : \/ Stop(r)
                       \/ GiveUp(r)
                       \/ Dispatches(r)
                       \/ Voids(r)
                       \/ Cuts(r)
                       \/ Skip(r)
                       \/ Resume(r)
    \/ \E id \in Ids : Expires(id)
    \/ Withdraw

Hurried(r) ==
    /\ Hurry
    /\ run[r].sends > 0
    /\ \/ gate[r] = "lost" /\ holds[Carries(r)].state # "new"
       \/ gate[r] = "reserved" /\ holds[Carries(r)].state # "held"

Next ==
    IF \E r \in Runs : Ended(r)
    THEN \E r \in Runs : Forget(r)
    ELSE IF \E r \in Runs : Hurried(r)
    THEN \E r \in Runs : /\ Hurried(r)
                          /\ \/ Resend(r, acct.cap)
                             \/ GiveUp(r)
                             \/ Skip(r)
                             \/ Dispatches(r)
                             \/ Voids(r)
    ELSE Moves

HoldsOf(r, k) == {id \in saw.ids[r][k] : saw.path[id] # <<>>}

S1 == "none" \notin saw.under

S2 == \A id \in Ids : \A i \in DOMAIN saw.path[id] :
          saw.path[id][i] = "sent" => \E j \in 1..(i - 1) : saw.path[id][j] = "held"

S3 == \A r \in Runs, k \in Requests : saw.calls[r][k] <= 1

S4 == last.kind = "reserve" => last.sum <= last.bound

S5 == \A r \in Runs, k \in Requests : Cardinality(HoldsOf(r, k)) <= 1

S6 == last.kind = "admission" => last.sum <= last.bound

S7 == \A r \in Runs :
          Cardinality({id \in UNION {HoldsOf(r, k) : k \in Requests} :
                          saw.path[id][Len(saw.path[id])] \in {"held", "sent"}}) <= 1

S8 == \A r \in Runs : saw.trail[r] # "sent_refused"

S9 == \A r \in Runs, k \in Requests : Cardinality(saw.amounts[r][k]) <= 1

S10 == \A r \in Runs, k \in Requests : saw.calls[r][k] > 0 => ~saw.unadmitted[r][k]

S11 == \A r \in Runs, k \in Requests : saw.sends[r][k] <= 2

Word(value) == IF value = Null THEN "-" ELSE ToString(value)

Opening(a) ==
    "cap=" \o Word(a.cap)
        \o " active=" \o (IF a.active THEN "yes" ELSE "no")
        \o " spent=" \o ToString(a.spent)
        \o " held=" \o ToString(a.held)

Token(m, res) ==
    " " \o m.name
        \o ":" \o Word(m.arg)
        \o ":" \o res.answer
        \o ":" \o res.hold.state
        \o ":" \o ToString(res.acct.spent)
        \o ":" \o ToString(res.acct.held)
        \o ":" \o ToString(res.hold.cost)

RECURSIVE Walked(_, _, _)
Walked(a, h, moves) ==
    IF moves = <<>> THEN ""
    ELSE LET res == Step(a, h, Head(moves))
         IN Token(Head(moves), res) \o Walked(res.acct, res.hold, Tail(moves))

Start(cap, spent, held) == [cap |-> cap, active |-> TRUE, spent |-> spent, held |-> held]

Settled(amount, cost) ==
    <<ToReserve(amount), ToDispatch, ToSettle(cost), ToReserve(amount), ToSettle(cost + 1)>>

Directed ==
    <<[acct |-> Start(3, 0, 1), moves |-> Settled(2, 1)],
      [acct |-> Start(3, 0, 0), moves |-> Settled(3, 2)],
      [acct |-> Start(3, 1, 0), moves |-> Settled(2, 2)],
      [acct |-> Start(1, 0, 0), moves |-> Settled(1, 1)],
      [acct |-> Start(2, 0, 0), moves |-> Settled(2, 0)],
      [acct |-> Start(3, 2, 0), moves |-> Settled(1, 0)]>>

XInit ==
    /\ \A i \in DOMAIN Directed :
           PrintT("directed " \o ToString(i) \o " " \o Opening(Directed[i].acct)
                      \o Walked(Directed[i].acct, NewHold, Directed[i].moves))
    /\ acct \in [cap : Caps \cup {Null}, active : BOOLEAN, spent : 0..2, held : 0..2]
    /\ holds = <<NewHold>>
    /\ tape = Opening(acct)
    /\ draw = [moves |-> 0, arg |-> 0]
    /\ ruling = "none"
    /\ budget = 0
    /\ run = 0
    /\ gate = 0
    /\ coord = 0
    /\ saw = 0
    /\ last = Blank

XNext ==
    \E name \in MoveNames, n \in 0..3 :
        LET m == [name |-> name, arg |-> IF name \in {"reserve", "settle"} THEN n ELSE Null]
            res == Step(acct, holds[1], m)
        IN /\ acct' = res.acct
           /\ holds' = <<res.hold>>
           /\ tape' = tape \o Token(m, res)
           /\ draw' = [moves |-> draw.moves + 1, arg |-> n]
           /\ UNCHANGED <<ruling, budget, run, gate, coord, saw, last>>

XTold == draw.moves < TraceMoves \/ PrintT("trace " \o tape)

=============================================================================
