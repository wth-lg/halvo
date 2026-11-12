------------------------------- MODULE Reach -------------------------------
EXTENDS Spend

ReachRulingNone == ruling # "none"
ReachRulingHosted == ruling # "hosted"
ReachTwoInFlight == \E r \in Runs : run[r].sends = 0
ReachTwoRequests ==
    \A r \in Runs : ~(run[r].made = 2 /\ \E id \in saw.ids[r][2] : holds[id].state = "settled")
ReachSecondSend == \A r \in Runs : run[r].sends < 2
ReachSecondSendServed ==
    \A r \in Runs : ~(run[r].sends = 2 /\ gate[r] = "upstream")
ReachLostWithHold ==
    \A r \in Runs : ~(gate[r] = "lost" /\ holds[Carries(r)].state # "new")
ReachStopWaits == \A r \in Runs : ~(run[r].stop /\ run[r].sends > 0)
ReachCapTwice == budget.caps > 0
ReachOtherSpent == budget.other = MaxOtherSpends
ReachOtherTwice == budget.other > 0
ReachSettled == \A id \in Ids : holds[id].state # "settled"
ReachExpired == \A id \in Ids : holds[id].state # "expired"
ReachVoided == \A id \in Ids : holds[id].state # "voided"
ReachOverCap == \A r \in Runs : ~run[r].waits
ReachRepeat ==
    \A r \in Runs :
        ~(/\ gate[r] = "lost"
          /\ run[r].sends = 1
          /\ Step(acct, holds[IdOf(r, run[r].made, 2)], ToReserve(run[r].amount)).answer = "repeat")
ReachUsed ==
    \A r \in Runs :
        ~(gate[r] = "reserved" /\ Step(acct, holds[Carries(r)], ToDispatch).answer = "used")
ReachState ==
    \A r \in Runs :
        ~(gate[r] = "reserved" /\ Step(acct, holds[Carries(r)], ToVoid).answer = "state")
ReachHoldOpenRunOff ==
    \A r \in Runs : run[r].on \/ OpenHolds(r) = {}

ReachSecondAdmission ==
    [][~(\E r \in Runs, e \in Estimates : run[r].admits = 1 /\ Admit(r, e))]_vars
ReachStaleOne ==
    [][~(last.kind = "admission" /\ \E r \in Runs : Admit(r, 1))]_vars
ReachStaleTwo ==
    [][~(last.kind = "admission" /\ \E r \in Runs : Admit(r, 2))]_vars
ReachAdmitAfterOverCap ==
    [][~(\E r \in Runs, e \in Estimates : run[r].waits /\ Admit(r, e))]_vars
ReachAdmitAboveRoomInForce ==
    [][~(\E r \in Runs, e \in Estimates :
            /\ Admit(r, e)
            /\ budget.caps < MaxCapChanges
            /\ e + SumOver(saw.live, Runs \ {r}) > Left(acct))]_vars

Reaching(r) == {cap \in Caps : InForce(cap)}

SendAnswers(r) ==
    IF run[r].on /\ run[r].sends = 0 /\ gate[r] = "idle" /\ run[r].made < MaxRequests
       /\ ~run[r].stop /\ Grant(r)
    THEN {Step(Under(cap), holds[IdOf(r, run[r].made + 1, 1)], ToReserve(amount)).answer :
              amount \in Amounts, cap \in Reaching(r)}
    ELSE {}

ResendAnswers(r) ==
    IF gate[r] = "lost" /\ run[r].sends = 1 /\ Grant(r)
    THEN {Step(Under(cap), holds[IdOf(r, run[r].made, 2)], ToReserve(run[r].amount)).answer :
              cap \in Reaching(r)}
    ELSE {}

GateAnswers(r) ==
    IF gate[r] = "reserved"
    THEN {Step(acct, holds[Carries(r)], ToDispatch).answer,
          Step(acct, holds[Carries(r)], ToVoid).answer}
    ELSE IF gate[r] = "upstream"
    THEN {Step(acct, holds[Carries(r)], ToSettle(cost)).answer :
              cost \in {c \in Amounts : c <= holds[Carries(r)].amount}}
    ELSE {}

AnswersNow ==
    UNION {SendAnswers(r) \cup ResendAnswers(r) \cup GateAnswers(r) : r \in Runs}
        \cup {Step(acct, holds[id], ToExpire).answer :
                  id \in {i \in Ids : holds[i].state \in {"held", "sent"}}}

OnlyFourCauses == AnswersNow \subseteq {"ok", "over_cap", "repeat", "used", "state"}

TypeOK ==
    /\ ruling \in {"none", "hosted"}
    /\ acct.cap \in Caps /\ acct.active /\ acct.spent \in 0..9 /\ acct.held \in 0..3
    /\ acct.held = SumOver([id \in Ids |-> IF holds[id].state \in {"held", "sent"}
                                           THEN holds[id].amount ELSE 0], Ids)
    /\ \A id \in Ids : holds[id].state \in States /\ holds[id].amount \in 0..2
    /\ \A r \in Runs :
        /\ run[r].admits \in 0..MaxAdmissions /\ run[r].made \in 0..MaxRequests
        /\ run[r].sends \in 0..MaxSends
        /\ gate[r] \in {"idle", "reserved", "upstream", "lost"}
        /\ (gate[r] # "idle") => run[r].sends > 0
        /\ run[r].on = (coord.list[r] > 0)
        /\ coord.list[r] = saw.live[r]
        /\ run[r].waits = (saw.trail[r] = "refused")
    /\ coord.room = saw.room

=============================================================================
