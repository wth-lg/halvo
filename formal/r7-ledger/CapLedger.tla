---------------------------- MODULE CapLedger ----------------------------
EXTENDS Integers, FiniteSets, TLC

CONSTANTS Ids, NoId, Amounts, Costs, CapAmounts, MaxTime, Life, WeekLen, MonthLen,
          IntervalLen, PeriodTypes, ClockFallsBack, FollowsAGateway, History

VARIABLES clock, o, gw, last, hist

vars == <<clock, o, gw, last, hist>>

NoTime == -1
NoCost == -1
Types == {"week", "month"}
Ended == {"settled", "voided", "expired"}
CallKinds == {"reserve", "dispatch", "settle", "void", "expire", "setcap", "capread"}

NoHold == [state |-> "none", amount |-> 0, period |-> 0, interval |-> 0, expiresAt |-> 0,
           mark |-> FALSE, cost |-> NoCost, overrun |-> 0, late |-> FALSE]
NoCap == [set |-> FALSE, amount |-> 0, type |-> "none", anchor |-> 0, next |-> "none",
          switchAt |-> NoTime]
EmptyObject == [lastNow |-> 0, cap |-> NoCap, holds |-> [r \in Ids |-> NoHold],
                totals |-> [p \in {} |-> NoHold], alarm |-> NoTime, goes |-> [r \in Ids |-> 0]]

Later(a, b) == IF a > b THEN a ELSE b
Above(cost, amount) == IF cost > amount THEN cost - amount ELSE 0

RECURSIVE SumOver(_, _)
SumOver(f, S) ==
    IF S = {} THEN 0
    ELSE LET x == CHOOSE y \in S : TRUE IN f[x] + SumOver(f, S \ {x})

Now(ob, given) == Later(given, ob.lastNow)
PeriodLen(type) == IF type = "week" THEN WeekLen ELSE MonthLen
PeriodStart(cap, now) == now - ((now - cap.anchor) % PeriodLen(cap.type))
IntervalOf(now) == now - (now % IntervalLen)
OpenIds(ob) == {r \in Ids : ob.holds[r].state = "open"}
NextDue(ob) ==
    IF OpenIds(ob) = {} THEN NoTime
    ELSE CHOOSE t \in {ob.holds[r].expiresAt : r \in OpenIds(ob)} :
            \A r \in OpenIds(ob) : t <= ob.holds[r].expiresAt

SwitchPeriodType(ob, now) ==
    IF ob.cap.set /\ ob.cap.next # "none" /\ ob.cap.switchAt <= now
    THEN [ob EXCEPT !.cap.type = ob.cap.next, !.cap.anchor = ob.cap.switchAt,
                    !.cap.next = "none", !.cap.switchAt = NoTime]
    ELSE ob

ExpireDue(ob, now) ==
    LET due == {r \in Ids : ob.holds[r].state = "open" /\ ob.holds[r].expiresAt <= now}
        amount == [r \in Ids |-> ob.holds[r].amount]
    IN IF due = {} THEN ob
       ELSE [ob EXCEPT
               !.holds = [r \in Ids |-> IF r \in due THEN [ob.holds[r] EXCEPT !.state = "expired"]
                                        ELSE ob.holds[r]],
               !.totals = [p \in DOMAIN ob.totals |->
                             LET moved == SumOver(amount, {r \in due : ob.holds[r].period = p})
                             IN [ob.totals[p] EXCEPT !.open = @ - moved, !.unreported = @ + moved]]]

Pre(ob, given) ==
    LET now == Now(ob, given)
    IN ExpireDue(SwitchPeriodType([ob EXCEPT !.lastNow = now], now), now)

NoAnswer == [res |-> "none", reason |-> "none", state |-> "none", mark |-> FALSE,
             expiresAt |-> NoTime, cost |-> NoCost, overrun |-> 0, repeat |-> FALSE,
             amount |-> 0, type |-> "none", spent |-> 0, held |-> 0, periodEnd |-> NoTime]
Refused(why) == [NoAnswer EXCEPT !.res = "refused", !.reason = why]
Unchanged(p, answer) == [o |-> p, ans |-> answer]

RowOf(ob, start) ==
    IF start \in DOMAIN ob.totals THEN ob.totals[start]
    ELSE [end |-> start + PeriodLen(ob.cap.type), reported |-> 0, unreported |-> 0, open |-> 0]

CapAnswer(ob, now) ==
    IF ~ob.cap.set THEN [NoAnswer EXCEPT !.res = "no-cap"]
    ELSE LET start == PeriodStart(ob.cap, now)
             row == RowOf(ob, start)
         IN [NoAnswer EXCEPT !.res = "cap", !.amount = ob.cap.amount,
                             !.type = IF ob.cap.next # "none" THEN ob.cap.next ELSE ob.cap.type,
                             !.spent = row.reported + row.unreported, !.held = row.open,
                             !.periodEnd = start + PeriodLen(ob.cap.type)]

OwnSetCap(p, now, mv) ==
    LET cap == p.cap
        set == IF ~cap.set
               THEN [set |-> TRUE, amount |-> mv.cap, type |-> mv.type, anchor |-> now,
                     next |-> "none", switchAt |-> NoTime]
               ELSE IF mv.type = cap.type
               THEN [cap EXCEPT !.amount = mv.cap, !.next = "none", !.switchAt = NoTime]
               ELSE [cap EXCEPT !.amount = mv.cap, !.next = mv.type,
                                !.switchAt = IF cap.switchAt # NoTime THEN cap.switchAt
                                             ELSE PeriodStart(cap, now) + PeriodLen(cap.type)]
        q == [p EXCEPT !.cap = set]
    IN [o |-> q, ans |-> CapAnswer(q, now)]

OwnReserve(p, now, mv) ==
    LET h == p.holds[mv.r]
    IN IF h.state # "none"
       THEN Unchanged(p, IF h.amount = mv.amt
                         THEN [NoAnswer EXCEPT !.res = "repeat", !.state = h.state, !.mark = h.mark,
                                               !.amount = h.amount, !.expiresAt = h.expiresAt]
                         ELSE Refused("other-arguments"))
       ELSE IF ~mv.active THEN Unchanged(p, Refused("not-active"))
       ELSE IF ~p.cap.set THEN Unchanged(p, Refused("no-cap"))
       ELSE LET start == PeriodStart(p.cap, now)
                row == RowOf(p, start)
                counted == row.reported + row.unreported + row.open
            IN IF counted + mv.amt > p.cap.amount
               THEN Unchanged(p, Refused("over-cap"))
               ELSE [o |-> [p EXCEPT
                              !.totals = [s \in DOMAIN p.totals \cup {start} |->
                                            IF s = start THEN [row EXCEPT !.open = @ + mv.amt]
                                            ELSE p.totals[s]],
                              !.holds[mv.r] = [state |-> "open", amount |-> mv.amt, period |-> start,
                                               interval |-> IntervalOf(now), expiresAt |-> now + Life,
                                               mark |-> FALSE, cost |-> NoCost, overrun |-> 0,
                                               late |-> FALSE]],
                     ans |-> [NoAnswer EXCEPT !.res = "admitted", !.amount = mv.amt,
                                              !.expiresAt = now + Life]]

OwnDispatch(p, mv) ==
    LET h == p.holds[mv.r]
    IN IF h.state = "open" /\ ~h.mark
       THEN [o |-> [p EXCEPT !.holds[mv.r].mark = TRUE, !.goes[mv.r] = @ + 1],
             ans |-> [NoAnswer EXCEPT !.res = "go"]]
       ELSE Unchanged(p, [NoAnswer EXCEPT !.res = "stop"])

OwnSettle(p, mv) ==
    LET h == p.holds[mv.r]
        outcome(res, repeat) == [NoAnswer EXCEPT !.res = res, !.repeat = repeat,
                                                 !.cost = IF repeat THEN h.cost ELSE mv.cost,
                                                 !.overrun = IF repeat THEN h.overrun
                                                             ELSE Above(mv.cost, h.amount)]
    IN CASE h.state = "none" -> Unchanged(p, Refused("unknown-request"))
         [] h.state = "open" ->
               [o |-> [p EXCEPT !.holds[mv.r] = [h EXCEPT !.state = "settled", !.cost = mv.cost,
                                                          !.overrun = Above(mv.cost, h.amount)],
                                !.totals[h.period] = [@ EXCEPT !.open = @ - h.amount,
                                                               !.reported = @ + mv.cost]],
                ans |-> outcome("settled", FALSE)]
         [] h.state = "settled" -> Unchanged(p, outcome("settled", TRUE))
         [] h.state = "expired" ->
               IF h.late THEN Unchanged(p, [outcome("expired", TRUE) EXCEPT !.repeat = FALSE])
               ELSE [o |-> [p EXCEPT !.holds[mv.r] = [h EXCEPT !.cost = mv.cost, !.late = TRUE,
                                                               !.overrun = Above(mv.cost, h.amount)]],
                     ans |-> outcome("expired", FALSE)]
         [] h.state = "voided" -> Unchanged(p, Refused("already-ended"))

OwnVoid(p, mv) ==
    LET h == p.holds[mv.r]
    IN CASE h.state = "none" -> Unchanged(p, Refused("unknown-request"))
         [] h.state = "voided" -> Unchanged(p, [NoAnswer EXCEPT !.res = "voided"])
         [] h.state \in {"settled", "expired"} -> Unchanged(p, Refused("already-ended"))
         [] h.state = "open" ->
               IF h.mark THEN Unchanged(p, Refused("dispatched"))
               ELSE [o |-> [p EXCEPT !.holds[mv.r].state = "voided",
                                     !.totals[h.period].open = @ - h.amount],
                     ans |-> [NoAnswer EXCEPT !.res = "voided"]]

OwnExpire(ob, p, mv) ==
    LET before == ob.holds[mv.r]
        h == p.holds[mv.r]
    IN Unchanged(p, IF h.state = "none" THEN Refused("unknown-request")
                    ELSE IF before.state = "open" /\ h.state = "expired"
                    THEN [NoAnswer EXCEPT !.res = "expired"]
                    ELSE IF h.state = "open" THEN Refused("not-due")
                    ELSE Refused("already-ended"))

Step(ob, given, mv) ==
    LET now == Now(ob, given)
        p == Pre(ob, given)
        done == CASE mv.k = "reserve" -> OwnReserve(p, now, mv)
                  [] mv.k = "dispatch" -> OwnDispatch(p, mv)
                  [] mv.k = "settle" -> OwnSettle(p, mv)
                  [] mv.k = "void" -> OwnVoid(p, mv)
                  [] mv.k = "expire" -> OwnExpire(ob, p, mv)
                  [] mv.k = "setcap" -> OwnSetCap(p, now, mv)
                  [] mv.k = "capread" -> Unchanged(p, CapAnswer(p, now))
    IN [o |-> [done.o EXCEPT !.alarm = NextDue(done.o)], ans |-> done.ans, pre |-> p]

Rearm(ob, given) == [ob EXCEPT !.lastNow = Now(ob, given), !.alarm = NextDue(ob)]

Idle == [r |-> NoId, pc |-> "idle"]

GatewayAfter(g, mv, ans) ==
    IF ~mv.own THEN g
    ELSE CASE mv.k = "reserve" ->
                 IF ans.res = "admitted" \/ (ans.res = "repeat" /\ ans.state = "open" /\ ~ans.mark)
                 THEN [r |-> mv.r, pc |-> "admitted"]
                 ELSE Idle
           [] mv.k = "dispatch" -> IF ans.res = "go" THEN [r |-> mv.r, pc |-> "upstream"] ELSE Idle
           [] OTHER -> Idle

Move(k, own, r) == [k |-> k, own |-> own, r |-> r, amt |-> 0, cost |-> 0, active |-> TRUE,
                    cap |-> 0, type |-> "none"]
Reserves(own, facts) == {[Move("reserve", own, r) EXCEPT !.amt = a, !.active = b] :
                           r \in Ids, a \in Amounts, b \in facts}
Settles(own, R) == {[Move("settle", own, r) EXCEPT !.cost = c] : r \in R, c \in Costs}
CallsOfAnySender ==
    Reserves(FALSE, BOOLEAN)
    \cup {Move("dispatch", FALSE, r) : r \in Ids}
    \cup Settles(FALSE, Ids)
    \cup {Move("void", FALSE, r) : r \in Ids}
    \cup {Move("expire", FALSE, r) : r \in Ids}
    \cup {[Move("setcap", FALSE, NoId) EXCEPT !.cap = c, !.type = t] :
            c \in CapAmounts, t \in PeriodTypes}
    \cup {Move("capread", FALSE, NoId)}
CallsOfTheGateway(g) ==
    CASE g.pc = "idle" -> IF FollowsAGateway THEN Reserves(TRUE, {TRUE}) ELSE {}
      [] g.pc = "admitted" -> {Move("dispatch", TRUE, g.r), Move("void", TRUE, g.r)}
      [] g.pc = "upstream" -> Settles(TRUE, {g.r})

NoHistory == [early |-> {}, refused |-> {}, restarted |-> {}, crashed |-> {}, expired |-> {},
              lastSetCap |-> 0]
NoLast == [mv |-> Move("none", FALSE, NoId), ans |-> NoAnswer, was |-> "none"]

Init ==
    /\ clock = 0
    /\ o = EmptyObject
    /\ gw = Idle
    /\ last = NoLast
    /\ hist = NoHistory

Deliver(mv) ==
    LET done == Step(o, clock, mv)
    IN /\ o' = done.o
       /\ gw' = GatewayAfter(gw, mv, done.ans)
       /\ last' = [mv |-> mv, ans |-> done.ans, was |-> done.pre.holds[mv.r].state]
       /\ hist' = IF ~History THEN hist
                  ELSE [hist EXCEPT
                          !.early = IF mv.k = "settle" /\ done.ans.reason = "unknown-request"
                                    THEN @ \cup {mv.r} ELSE @,
                          !.refused = IF mv.k = "reserve" /\ done.ans.reason = "over-cap"
                                      THEN @ \cup {<<mv.r, mv.amt>>} ELSE @,
                          !.expired = IF mv.k = "expire" /\ done.ans.res = "expired"
                                      THEN @ \cup {mv.r} ELSE @]
       /\ UNCHANGED clock

DeliverToNoHold(mv) ==
    LET done == Step(o, clock, mv)
    IN /\ o' = done.o
       /\ gw' = gw
       /\ last' = [mv |-> mv, ans |-> done.ans, was |-> "none"]
       /\ hist' = IF mv.k = "setcap" THEN [hist EXCEPT !.lastSetCap = mv.cap] ELSE hist
       /\ UNCHANGED clock

GatewayStops ==
    /\ gw.pc # "idle"
    /\ gw' = Idle
    /\ last' = [NoLast EXCEPT !.mv = Move("stop", TRUE, gw.r)]
    /\ hist' = IF History /\ gw.pc = "upstream" THEN [hist EXCEPT !.crashed = @ \cup {gw.r}]
               ELSE hist
    /\ UNCHANGED <<clock, o>>

Restart(behind) ==
    /\ o' = Rearm(o, clock)
    /\ clock' = IF behind THEN o'.lastNow - 1 ELSE clock
    /\ last' = [NoLast EXCEPT !.mv = Move(IF behind THEN "back" ELSE "restart", FALSE, NoId)]
    /\ hist' = IF ~History THEN hist
               ELSE [hist EXCEPT !.restarted = @ \cup {r \in Ids : o.holds[r].state = "open"
                                                                  /\ o.holds[r].mark}]
    /\ UNCHANGED gw

Tick ==
    /\ o.cap.set
    /\ clock < MaxTime
    /\ clock' = clock + 1
    /\ last' = [NoLast EXCEPT !.mv = Move("tick", FALSE, NoId)]
    /\ UNCHANGED <<o, gw, hist>>

Next ==
    \/ \E mv \in CallsOfTheGateway(gw) : Deliver(mv)
    \/ \E mv \in CallsOfAnySender : IF mv.r = NoId THEN DeliverToNoHold(mv) ELSE Deliver(mv)
    \/ GatewayStops
    \/ \E behind \in (IF ClockFallsBack THEN BOOLEAN ELSE {FALSE}) : Restart(behind)
    \/ Tick

TimeOfTheNextStep == Later(clock, o.lastNow)
CapOfTheNextStep == SwitchPeriodType(o, TimeOfTheNextStep).cap
PeriodOfTheNextStep == PeriodStart(CapOfTheNextStep, TimeOfTheNextStep)
FromNow(time) == IF time > TimeOfTheNextStep THEN time - TimeOfTheNextStep ELSE 0
HoldInView(h) ==
    CASE h.state = "none" -> <<"none">>
      [] h.state = "open" -> <<"open", h.amount, h.mark, FromNow(h.expiresAt),
                               h.period = PeriodOfTheNextStep, h.cost, h.late, h.overrun,
                               h.interval = IntervalOf(TimeOfTheNextStep)>>
      [] OTHER -> <<h.state, h.amount, h.mark, h.cost, h.late, h.overrun>>
CapInView ==
    IF ~o.cap.set THEN <<"none">>
    ELSE <<CapOfTheNextStep.amount, CapOfTheNextStep.type, TimeOfTheNextStep - PeriodOfTheNextStep,
           CapOfTheNextStep.next, FromNow(CapOfTheNextStep.switchAt)>>
RowInView ==
    IF o.cap.set /\ PeriodOfTheNextStep \in DOMAIN o.totals
    THEN <<o.totals[PeriodOfTheNextStep].reported, o.totals[PeriodOfTheNextStep].unreported,
           o.totals[PeriodOfTheNextStep].open>>
    ELSE <<"none">>
SlotInView(r) == <<HoldInView(o.holds[r]), IF gw.r = r THEN gw.pc ELSE "idle", o.goes[r]>>
HoldsInView == [slot \in {SlotInView(r) : r \in Ids} |->
                  Cardinality({r \in Ids : SlotInView(r) = slot})]
View == <<TimeOfTheNextStep - clock, CapInView, RowInView, HoldsInView,
          IF o.alarm = NoTime THEN NoTime ELSE FromNow(o.alarm), hist,
          TimeOfTheNextStep % IntervalLen>>

HoldOK(h) ==
    /\ h.state \in {"none", "open"} \cup Ended
    /\ h.amount \in Amounts \cup {0}
    /\ h.period \in Nat
    /\ h.interval \in Nat
    /\ h.expiresAt \in Nat
    /\ h.mark \in BOOLEAN
    /\ h.cost \in Costs \cup {NoCost}
    /\ h.overrun \in Nat
    /\ h.late \in BOOLEAN
    /\ h.state = "none" => h = NoHold

TypeOK ==
    /\ clock \in Int
    /\ clock >= -1
    /\ clock <= MaxTime
    /\ o.lastNow \in Nat
    /\ o.lastNow <= clock + 1
    /\ o.cap.set \in BOOLEAN
    /\ o.cap.set => /\ o.cap.amount \in CapAmounts
                    /\ o.cap.type \in Types
                    /\ o.cap.anchor \in Nat
                    /\ o.cap.next \in Types \cup {"none"}
                    /\ (o.cap.next = "none") = (o.cap.switchAt = NoTime)
    /\ ~o.cap.set => o.cap = NoCap
    /\ \A r \in Ids : HoldOK(o.holds[r]) /\ o.goes[r] \in Nat
    /\ DOMAIN o.totals \subseteq Nat
    /\ \A p \in DOMAIN o.totals : /\ o.totals[p].end > p
                                  /\ o.totals[p].reported \in Nat
                                  /\ o.totals[p].unreported \in Nat
                                  /\ o.totals[p].open \in Nat
    /\ o.alarm = NoTime \/ o.alarm \in Nat
    /\ gw.pc \in {"idle", "admitted", "upstream"}
    /\ (gw.pc = "idle") = (gw.r = NoId)
    /\ gw.pc # "idle" => gw.r \in Ids

Has(ob, r) == ob.holds[r].state # "none"
HoldsIn(ob, p) == {r \in Ids : Has(ob, r) /\ ob.holds[r].period = p}
CostOf(ob) == [r \in Ids |-> ob.holds[r].cost]
AmountOf(ob) == [r \in Ids |-> ob.holds[r].amount]
In(ob, S, state) == {r \in S : ob.holds[r].state = state}
ReportedOf(ob, p) == SumOver(CostOf(ob), In(ob, HoldsIn(ob, p), "settled"))
UnreportedOf(ob, p) == SumOver(AmountOf(ob), In(ob, HoldsIn(ob, p), "expired"))
OpenOf(ob, p) == SumOver(AmountOf(ob), In(ob, HoldsIn(ob, p), "open"))
CountedSpend(ob, p) == ReportedOf(ob, p) + UnreportedOf(ob, p) + OpenOf(ob, p)
PeriodsWithHolds(ob) == {ob.holds[r].period : r \in {x \in Ids : Has(ob, x)}}

Mv == last'.mv
Ans == last'.ans
StepTime == Later(clock, o.lastNow)
IsACall == Mv.k \in CallKinds
NamesAHold == Mv.k \in {"reserve", "dispatch", "settle", "void", "expire"}
SomeHoldChanged == o'.holds # o.holds
SomeTotalChanged == o'.totals # o.totals
NoRow == [end |-> 0, reported |-> 0, unreported |-> 0, open |-> 0]
Row(ob, p) == IF p \in DOMAIN ob.totals THEN ob.totals[p] ELSE NoRow
CountedIn(row) == row.reported + row.unreported + row.open
CapInForce == hist.lastSetCap

AnAdmissionWithinTheCapInForce(p) ==
    /\ Mv.k = "reserve"
    /\ Ans.res = "admitted"
    /\ o'.holds[Mv.r].period = p
    /\ CountedIn(Row(o', p)) - CountedIn(Row(o, p)) = Mv.amt
    /\ CountedIn(Row(o', p)) <= CapInForce
ARecordedOverrun(p) ==
    /\ Mv.k = "settle"
    /\ Ans.res = "settled"
    /\ ~Ans.repeat
    /\ o'.holds[Mv.r].period = p
    /\ o'.holds[Mv.r].overrun > 0
    /\ CountedIn(Row(o', p)) - CountedIn(Row(o, p)) = o'.holds[Mv.r].overrun
CountedSpendRisesOnlyAtAnAdmissionOrByARecordedOverrun ==
    SomeTotalChanged =>
        \A p \in DOMAIN o'.totals :
            CountedIn(Row(o', p)) > CountedIn(Row(o, p)) =>
                AnAdmissionWithinTheCapInForce(p) \/ ARecordedOverrun(p)
OverrunsStayZeroWhileEveryReportedCostIsWithinItsHold ==
    \A r \in Ids :
        (o.holds[r].cost = NoCost \/ o.holds[r].cost <= o.holds[r].amount) => o.holds[r].overrun = 0
StateA == OverrunsStayZeroWhileEveryReportedCostIsWithinItsHold
StepA == [][CountedSpendRisesOnlyAtAnAdmissionOrByARecordedOverrun]_vars
PropA == []StateA /\ StepA

NoOpenHoldIsDueAfterAStepOfTheLedger ==
    IsACall => \A r \in OpenIds(o') : o'.holds[r].expiresAt > StepTime
TheDueTimeIsAtOrBeforeTheEarliestExpiry ==
    \A r \in OpenIds(o) : o.alarm # NoTime /\ o.alarm <= o.holds[r].expiresAt
NoHoldEndsByExpiryBeforeItsExpiry ==
    \A r \in Ids :
        o.holds[r].state = "open" /\ o'.holds[r].state = "expired" => o.holds[r].expiresAt <= StepTime
StateB == TheDueTimeIsAtOrBeforeTheEarliestExpiry
StepB == [][NoOpenHoldIsDueAfterAStepOfTheLedger /\ NoHoldEndsByExpiryBeforeItsExpiry]_vars
PropB == []StateB /\ StepB

AnEndedHoldKeepsItsEnd ==
    \A r \in Ids : o.holds[r].state \in Ended => o'.holds[r].state = o.holds[r].state
AHoldIsNeverRemovedOrMadeAgain ==
    \A r \in Ids :
        Has(o, r) => /\ Has(o', r)
                     /\ o'.holds[r].amount = o.holds[r].amount
                     /\ o'.holds[r].period = o.holds[r].period
                     /\ o'.holds[r].interval = o.holds[r].interval
                     /\ o'.holds[r].expiresAt = o.holds[r].expiresAt
AHoldEndsOnce == SomeHoldChanged => AnEndedHoldKeepsItsEnd /\ AHoldIsNeverRemovedOrMadeAgain
StepC == [][AHoldEndsOnce]_vars
PropC == StepC

NothingChangedInTheCallsOwnWork(p) ==
    /\ o'.totals = p.totals
    /\ o'.holds = p.holds
NothingChangedButTheLateCostAndItsOverrun(p) ==
    LET h == p.holds[Mv.r]
    IN /\ o'.totals = p.totals
       /\ o'.holds = [p.holds EXCEPT ![Mv.r] = [h EXCEPT !.cost = Mv.cost, !.late = TRUE,
                                                         !.overrun = Above(Mv.cost, h.amount)]]
IsARepeat(h) ==
    \/ Mv.k = "reserve" /\ h.state # "none"
    \/ Mv.k = "dispatch" /\ h.mark
    \/ Mv.k = "settle" /\ (h.state = "settled" \/ (h.state = "expired" /\ h.late))
    \/ Mv.k = "void" /\ h.state = "voided"
    \/ Mv.k = "expire" /\ o.holds[Mv.r].state = "expired"
ARepeatChangesNoTotalAndNoHoldState(p) ==
    IsARepeat(p.holds[Mv.r]) => NothingChangedInTheCallsOwnWork(p)
TheSameCallAgainAtOnceChangesNothing ==
    LET again == Step(o', clock, Mv)
    IN again.o.totals = o'.totals /\ again.o.holds = o'.holds
ACallBeforeItsReserveIsRefused(p) ==
    Mv.k \in {"settle", "void", "expire"} /\ p.holds[Mv.r].state = "none" =>
        /\ Ans.res = "refused"
        /\ Ans.reason = "unknown-request"
        /\ NothingChangedInTheCallsOwnWork(p)
ACallAfterTheEndChangesNothingButTheOverrunThatSettleRecords(p) ==
    LET h == p.holds[Mv.r]
    IN Mv.k \in {"settle", "void", "expire"} /\ h.state \in Ended =>
          IF Mv.k = "settle" /\ h.state = "expired" /\ ~h.late
          THEN NothingChangedButTheLateCostAndItsOverrun(p)
          ELSE NothingChangedInTheCallsOwnWork(p)
InEveryCallD ==
    IsACall =>
        LET p == Pre(o, clock)
        IN /\ TheSameCallAgainAtOnceChangesNothing
           /\ NamesAHold => /\ ARepeatChangesNoTotalAndNoHoldState(p)
                            /\ ACallBeforeItsReserveIsRefused(p)
                            /\ ACallAfterTheEndChangesNothingButTheOverrunThatSettleRecords(p)
StepD == [][InEveryCallD]_vars
PropD == StepD

TheRequestIdRule(h) ==
    IF h.amount = Mv.amt
    THEN Ans.res = "repeat" /\ Ans.state = h.state /\ Ans.mark = h.mark
    ELSE Ans.res = "refused" /\ Ans.reason = "other-arguments"
WouldPassTheCap(p) ==
    CountedIn(Row(p, PeriodStart(p.cap, StepTime))) + Mv.amt > CapInForce
ARefusalForOneOfTheThreeReasonsAndNoOther(p) ==
    LET mustRefuse == ~Mv.active \/ ~p.cap.set \/ WouldPassTheCap(p)
    IN /\ Ans.res = (IF mustRefuse THEN "refused" ELSE "admitted")
       /\ Ans.reason = "not-active" => ~Mv.active
       /\ Ans.reason = "no-cap" => ~p.cap.set
       /\ Ans.reason = "over-cap" => p.cap.set /\ WouldPassTheCap(p)
       /\ Ans.res = "refused" => Ans.reason \in {"not-active", "no-cap", "over-cap"} /\ ~Has(o', Mv.r)
       /\ Ans.res = "admitted" => o'.holds[Mv.r].state = "open" /\ o'.holds[Mv.r].amount = Mv.amt
InEveryReserveE ==
    Mv.k = "reserve" =>
        LET p == Pre(o, clock)
        IN IF Has(p, Mv.r) THEN TheRequestIdRule(p.holds[Mv.r])
           ELSE ARefusalForOneOfTheThreeReasonsAndNoOther(p)
StepE == [][InEveryReserveE]_vars
PropE == StepE

ReportedBy(h) == IF h.state = "settled" THEN h.cost ELSE 0
UnreportedBy(h) == IF h.state = "expired" THEN h.amount ELSE 0
OpenBy(h) == IF h.state = "open" THEN h.amount ELSE 0
ATotalChangesOnlyByWhatAHoldAdds ==
    SomeTotalChanged \/ SomeHoldChanged =>
        /\ DOMAIN o.totals \subseteq DOMAIN o'.totals
        /\ \A r \in Ids : Has(o', r) => o'.holds[r].period \in DOMAIN o'.totals
        /\ \A p \in DOMAIN o'.totals :
              LET changed == {r \in Ids : o'.holds[r] # o.holds[r] /\ o'.holds[r].period = p}
                  reported == [r \in Ids |-> ReportedBy(o'.holds[r]) - ReportedBy(o.holds[r])]
                  unreported == [r \in Ids |-> UnreportedBy(o'.holds[r]) - UnreportedBy(o.holds[r])]
                  open == [r \in Ids |-> OpenBy(o'.holds[r]) - OpenBy(o.holds[r])]
              IN /\ o'.totals[p].reported - Row(o, p).reported = SumOver(reported, changed)
                 /\ o'.totals[p].unreported - Row(o, p).unreported = SumOver(unreported, changed)
                 /\ o'.totals[p].open - Row(o, p).open = SumOver(open, changed)
EveryTotalIsWhatItsHoldsAdd ==
    /\ DOMAIN o.totals = PeriodsWithHolds(o)
    /\ \A p \in DOMAIN o.totals :
          /\ o.totals[p].reported = ReportedOf(o, p)
          /\ o.totals[p].unreported = UnreportedOf(o, p)
          /\ o.totals[p].open = OpenOf(o, p)
ASettledHoldCarriesTheCostOfItsFirstSettle ==
    /\ Mv.k = "settle" /\ Ans.res = "settled" /\ ~Ans.repeat =>
          /\ Pre(o, clock).holds[Mv.r].state = "open"
          /\ o'.holds[Mv.r].state = "settled"
          /\ o'.holds[Mv.r].cost = Mv.cost
    /\ SomeHoldChanged =>
          \A r \in Ids : o.holds[r].state = "settled" =>
              o'.holds[r].cost = o.holds[r].cost /\ o'.holds[r].overrun = o.holds[r].overrun
StateF == EveryTotalIsWhatItsHoldsAdd
StepF == [][ATotalChangesOnlyByWhatAHoldAdds /\ ASettledHoldCarriesTheCostOfItsFirstSettle]_vars
PropF == []StateF /\ StepF

ADispatchHappensOnlyUnderAnOpenHold ==
    /\ Mv.k = "dispatch" /\ Ans.res = "go" =>
          LET h == Pre(o, clock).holds[Mv.r]
          IN h.state = "open" /\ ~h.mark /\ o'.holds[Mv.r].state = "open" /\ o'.holds[Mv.r].mark
    /\ SomeHoldChanged =>
          \A r \in Ids :
              /\ o.holds[r].mark => o'.holds[r].mark
              /\ ~o.holds[r].mark /\ o'.holds[r].mark => Mv.k = "dispatch" /\ Mv.r = r /\ Ans.res = "go"
AtMostOncePerHold ==
    \A r \in Ids : o.goes[r] <= 1 /\ o.holds[r].mark = (o.goes[r] > 0)
ADispatchedHoldNeverEndsAsVoid ==
    \A r \in Ids : o.holds[r].state = "voided" => ~o.holds[r].mark
TheGatewayCallsUpstreamOnlyOnGo ==
    gw'.pc = "upstream" /\ gw # gw' =>
        Mv.k = "dispatch" /\ Mv.own /\ Ans.res = "go" /\ gw'.r = Mv.r /\ gw.pc = "admitted"
TheGatewayIsUpstreamOnlyUnderAMarkedHold ==
    gw.pc = "upstream" => o.holds[gw.r].mark
StateG ==
    /\ AtMostOncePerHold
    /\ ADispatchedHoldNeverEndsAsVoid
    /\ TheGatewayIsUpstreamOnlyUnderAMarkedHold
StepG == [][ADispatchHappensOnlyUnderAnOpenHold /\ TheGatewayCallsUpstreamOnlyOnGo]_vars
PropG == []StateG /\ StepG

HoldsOfInterval(ob, i) == {r \in Ids : Has(ob, r) /\ ob.holds[r].interval = i}
IntervalsOf(ob) == {ob.holds[r].interval : r \in {x \in Ids : Has(ob, x)}}
IntervalTotals(ob, i) ==
    [reported |-> SumOver(CostOf(ob), In(ob, HoldsOfInterval(ob, i), "settled")),
     unreported |-> SumOver(AmountOf(ob), In(ob, HoldsOfInterval(ob, i), "expired")),
     open |-> Cardinality(In(ob, HoldsOfInterval(ob, i), "open"))]
CanBeSealed(ob, now, i) == i + IntervalLen <= now /\ IntervalTotals(ob, i).open = 0
NoTotalOfASealedIntervalChanges ==
    \A i \in IntervalsOf(o) \cup IntervalsOf(o') :
        CanBeSealed(o, StepTime, i) => IntervalTotals(o', i) = IntervalTotals(o, i)
AHoldJoinsOnlyTheIntervalOfItsStep ==
    \A r \in Ids :
        ~Has(o, r) /\ Has(o', r) =>
            o'.holds[r].interval <= StepTime /\ StepTime < o'.holds[r].interval + IntervalLen
AnEndedHoldAddsTheSameToItsIntervalForGood ==
    \A r \in Ids :
        o.holds[r].state \in Ended =>
            /\ o'.holds[r].state = o.holds[r].state
            /\ o'.holds[r].interval = o.holds[r].interval
            /\ o'.holds[r].amount = o.holds[r].amount
            /\ o.holds[r].state = "settled" => o'.holds[r].cost = o.holds[r].cost
WhenAHoldChangesH ==
    SomeHoldChanged => /\ NoTotalOfASealedIntervalChanges
                       /\ AHoldJoinsOnlyTheIntervalOfItsStep
                       /\ AnEndedHoldAddsTheSameToItsIntervalForGood
StepH == [][WhenAHoldChangesH]_vars
PropH == StepH

HoldOfLast == o.holds[last.mv.r]
ReachDuplicateReserve == last.mv.k = "reserve" /\ last.ans.res = "repeat"
ReachDuplicateDispatch == last.mv.k = "dispatch" /\ last.ans.res = "stop" /\ last.was = "open"
ReachDuplicateSettle == last.mv.k = "settle" /\ last.ans.res = "settled" /\ last.ans.repeat
ReachDuplicateVoid == last.mv.k = "void" /\ last.was = "voided"
ReachDuplicateExpire == last.mv.k = "expire" /\ last.ans.reason = "already-ended"
                        /\ last.mv.r \in hist.expired
ReachSettleBeforeItsReserve == \E r \in hist.early : Has(o, r)
ReachSettleAfterExpiry == last.mv.k = "settle" /\ last.ans.res = "expired" /\ HoldOfLast.late
ReachOverrun == \E r \in Ids : o.holds[r].state = "settled" /\ o.holds[r].overrun > 0
ReachCapBelowCountedSpendWithAHoldOpen ==
    /\ last.mv.k = "setcap"
    /\ \E p \in DOMAIN o.totals :
          /\ p = PeriodStart(o.cap, o.lastNow)
          /\ o.totals[p].open > 0
          /\ o.totals[p].reported + o.totals[p].unreported + o.totals[p].open > o.cap.amount
ReachChangeOfPeriodType == o.cap.set /\ o.cap.anchor > 0
ReachPeriodBoundaryWithAHoldOpen ==
    /\ last.mv.k \in CallKinds
    /\ \E r \in OpenIds(o) : o.holds[r].period < PeriodStart(o.cap, o.lastNow)
ReachRestartBetweenDispatchAndSettle == \E r \in hist.restarted : o.holds[r].state = "settled"
ReachGatewayCrashAfterDispatch == \E r \in hist.crashed : o.holds[r].state = "expired"
ReachVoidRefusedForADispatchedHold == last.mv.k = "void" /\ last.ans.reason = "dispatched"
ReachOpenHoldPastItsExpiryAfterARestart ==
    last.mv.k = "restart" /\ \E r \in OpenIds(o) : o.holds[r].expiresAt <= o.lastNow
ReachRefusedReserveAdmittedWhenSentAgain ==
    /\ last.mv.k = "reserve"
    /\ last.ans.res = "admitted"
    /\ <<last.mv.r, last.mv.amt>> \in hist.refused
ReachLateOverrunInNoTotal ==
    \E r \in Ids : o.holds[r].state = "expired" /\ o.holds[r].late /\ o.holds[r].overrun > 0

NoLateOverrunInNoTotal == ~ReachLateOverrunInNoTotal
NoDuplicateReserve == ~ReachDuplicateReserve
NoDuplicateDispatch == ~ReachDuplicateDispatch
NoDuplicateSettle == ~ReachDuplicateSettle
NoDuplicateVoid == ~ReachDuplicateVoid
NoDuplicateExpire == ~ReachDuplicateExpire
NoSettleBeforeItsReserve == ~ReachSettleBeforeItsReserve
NoSettleAfterExpiry == ~ReachSettleAfterExpiry
NoOverrun == ~ReachOverrun
NoCapBelowCountedSpendWithAHoldOpen == ~ReachCapBelowCountedSpendWithAHoldOpen
NoChangeOfPeriodType == ~ReachChangeOfPeriodType
NoPeriodBoundaryWithAHoldOpen == ~ReachPeriodBoundaryWithAHoldOpen
NoRestartBetweenDispatchAndSettle == ~ReachRestartBetweenDispatchAndSettle
NoGatewayCrashAfterDispatch == ~ReachGatewayCrashAfterDispatch
NoVoidRefusedForADispatchedHold == ~ReachVoidRefusedForADispatchedHold
NoOpenHoldPastItsExpiryAfterARestart == ~ReachOpenHoldPastItsExpiryAfterARestart
NoRefusedReserveAdmittedWhenSentAgain == ~ReachRefusedReserveAdmittedWhenSentAgain
=============================================================================
