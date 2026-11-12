------------------------- MODULE CapLedgerTrace -------------------------
EXTENDS CapLedger, Sequences

CONSTANTS Seed, Count, Depth, LastMonthStart

VARIABLE w

Quote == "\""
Str(x) == Quote \o x \o Quote
Num(n) == ToString(n)
OrNull(n) == IF n = -1 THEN "null" ELSE ToString(n)
Bool(b) == IF b THEN "true" ELSE "false"
Name(s) == IF s = "none" THEN "null" ELSE Str(s)
Id(r) == IF r = NoId THEN "null" ELSE Str("r" \o ToString(r))
KV(k, v) == Str(k) \o ":" \o v

RECURSIVE Join(_)
Join(xs) ==
    IF Len(xs) = 0 THEN ""
    ELSE IF Len(xs) = 1 THEN xs[1]
    ELSE xs[1] \o "," \o Join(Tail(xs))

Obj(xs) == "{" \o Join(xs) \o "}"

RECURSIVE Sorted(_)
Sorted(S) ==
    IF S = {} THEN <<>>
    ELSE LET m == CHOOSE x \in S : \A y \in S : x <= y
         IN <<m>> \o Sorted(S \ {m})

RECURSIVE Cat(_)
Cat(xss) == IF Len(xss) = 0 THEN <<>> ELSE xss[1] \o Cat(Tail(xss))

Lcg(x) ==
    LET t == 48271 * (x % 44488) - 3399 * (x \div 44488)
    IN IF t > 0 THEN t ELSE t + 2147483647

Draw(x, n) == (x \div 65536) % n

IdSeq == Sorted(Ids)
AmountSeq == Sorted(Amounts)
CostSeq == Sorted(Costs)
CapSeq == Sorted(CapAmounts)
TypeSeq == <<"week", "month">>
ClassSeq == <<"own", "reserve", "dispatch", "settle", "void", "expire", "clock", "setcap",
              "capread", "restart", "back", "stop">>
CallClasses == {"own", "reserve", "dispatch", "settle", "void", "expire", "setcap", "capread"}

Weight(c) ==
    CASE c = "own" -> 34
      [] c = "reserve" -> 8
      [] c = "dispatch" -> 4
      [] c = "settle" -> 7
      [] c = "void" -> 5
      [] c = "expire" -> 5
      [] c = "clock" -> 16
      [] c = "setcap" -> IF o.cap.set THEN 6 ELSE 60
      [] c = "capread" -> 4
      [] c = "restart" -> 4
      [] c = "back" -> 2
      [] c = "stop" -> 5

RECURSIVE WeightSum(_)
WeightSum(cs) == IF Len(cs) = 0 THEN 0 ELSE Weight(cs[1]) + WeightSum(Tail(cs))

RECURSIVE WeightPick(_, _)
WeightPick(cs, t) ==
    IF Len(cs) = 1 \/ t < Weight(cs[1]) THEN cs[1]
    ELSE WeightPick(Tail(cs), t - Weight(cs[1]))

WalkMove(c, mv, d) == [c |-> c, mv |-> mv, d |-> d]
NoCall(c) == Move(c, FALSE, NoId)
PerId(c, k) == [i \in 1..Len(IdSeq) |-> WalkMove(c, Move(k, FALSE, IdSeq[i]), 0)]
ReserveMoves(c, own) ==
    Cat([i \in 1..Len(IdSeq) |->
           [j \in 1..Len(AmountSeq) |->
              WalkMove(c, [Move("reserve", own, IdSeq[i]) EXCEPT !.amt = AmountSeq[j],
                                                                 !.active = w.active], 0)]])
SettleMoves(c, own, ids) ==
    Cat([i \in 1..Len(ids) |->
           [j \in 1..Len(CostSeq) |->
              WalkMove(c, [Move("settle", own, ids[i]) EXCEPT !.cost = CostSeq[j]], 0)]])
GatewayMoves ==
    CASE gw.pc = "idle" -> ReserveMoves("own", TRUE)
      [] gw.pc = "admitted" ->
            LET dispatch == WalkMove("own", Move("dispatch", TRUE, gw.r), 0)
            IN <<dispatch, dispatch, dispatch, WalkMove("own", Move("void", TRUE, gw.r), 0)>>
      [] gw.pc = "upstream" -> SettleMoves("own", TRUE, <<gw.r>>)

ToTheBoundary == PeriodStart(o.cap, clock) + PeriodLen(o.cap.type) - clock
FitsTheClock(m) == m.d >= 1 /\ clock + m.d <= MaxTime
ClockMoves ==
    IF ~o.cap.set THEN <<>>
    ELSE SelectSeq([i \in 1..6 |-> WalkMove("clock", NoCall("clock"),
                                            <<1, 1, 1, 2, 3, ToTheBoundary>>[i])],
                   FitsTheClock)

KeepsEveryMonthInFebruary(m) ==
    LET cap == Pre(o, clock).cap
        now == Now(o, clock)
        boundary == IF cap.switchAt # NoTime THEN cap.switchAt
                    ELSE PeriodStart(cap, now) + PeriodLen(cap.type)
    IN \/ ~cap.set
       \/ m.mv.type = cap.type
       \/ cap.type = "week" /\ boundary <= LastMonthStart
       \/ cap.type = "month" /\ PeriodStart(cap, now) = cap.anchor
SetCapMoves ==
    SelectSeq(Cat([i \in 1..Len(CapSeq) |->
                     [j \in 1..2 |->
                        WalkMove("setcap", [NoCall("setcap") EXCEPT !.cap = CapSeq[i],
                                                                    !.type = TypeSeq[j]], 0)]]),
              KeepsEveryMonthInFebruary)

MovesOf(c) ==
    CASE c = "own" -> GatewayMoves
      [] c = "reserve" -> ReserveMoves("reserve", FALSE)
      [] c = "dispatch" -> PerId("dispatch", "dispatch")
      [] c = "settle" -> SettleMoves("settle", FALSE, IdSeq)
      [] c = "void" -> PerId("void", "void")
      [] c = "expire" -> PerId("expire", "expire")
      [] c = "clock" -> ClockMoves
      [] c = "setcap" -> SetCapMoves
      [] c = "capread" -> <<WalkMove("capread", NoCall("capread"), 0)>>
      [] c = "restart" -> <<WalkMove("restart", NoCall("restart"), 0)>>
      [] c = "back" -> IF ClockFallsBack /\ clock > 0 /\ o.lastNow <= clock
                       THEN <<WalkMove("back", NoCall("back"), 0)>> ELSE <<>>
      [] c = "stop" -> IF gw.pc # "idle" THEN <<WalkMove("stop", Move("stop", TRUE, gw.r), 0)>>
                       ELSE <<>>

HasMoves(c) == Len(MovesOf(c)) > 0

Chosen(x1, x2) ==
    LET open == SelectSeq(ClassSeq, HasMoves)
        moves == MovesOf(WeightPick(open, Draw(x1, WeightSum(open))))
    IN moves[1 + Draw(x2, Len(moves))]

After(m) ==
    CASE m.c \in CallClasses ->
            LET done == Step(o, clock, m.mv)
            IN [clock |-> clock, o |-> done.o, gw |-> GatewayAfter(gw, m.mv, done.ans),
                ans |-> done.ans]
      [] m.c = "clock" ->
            LET t == clock + m.d
                due == NextDue(o) # NoTime /\ NextDue(o) <= Now(o, t)
            IN [clock |-> t,
                o |-> IF w.path = "host" THEN o
                      ELSE IF due THEN Step(o, t, Move("expire", FALSE, IdSeq[1])).o
                      ELSE Rearm(o, t),
                gw |-> gw, ans |-> NoAnswer]
      [] m.c = "back" -> [clock |-> Rearm(o, clock).lastNow - 1, o |-> Rearm(o, clock), gw |-> gw,
                          ans |-> NoAnswer]
      [] m.c = "restart" -> [clock |-> clock, o |-> Rearm(o, clock), gw |-> gw, ans |-> NoAnswer]
      [] m.c = "stop" -> [clock |-> clock, o |-> o, gw |-> Idle, ans |-> NoAnswer]

AnswerJson(a) ==
    Obj(<<KV("res", Str(a.res)), KV("reason", Name(a.reason)), KV("state", Name(a.state)),
          KV("mark", Bool(a.mark)), KV("expiresAt", OrNull(a.expiresAt)), KV("cost", OrNull(a.cost)),
          KV("overrun", Num(a.overrun)), KV("repeat", Bool(a.repeat)), KV("amount", Num(a.amount)),
          KV("type", Name(a.type)), KV("spent", Num(a.spent)), KV("held", Num(a.held)),
          KV("periodEnd", OrNull(a.periodEnd))>>)

HoldJson(h) ==
    Obj(<<KV("state", Str(h.state)), KV("amount", Num(h.amount)), KV("period", Num(h.period)),
          KV("interval", Num(h.interval)), KV("expiresAt", Num(h.expiresAt)), KV("mark", Bool(h.mark)),
          KV("cost", OrNull(h.cost)), KV("overrun", Num(h.overrun)), KV("late", Bool(h.late))>>)

CapJson(c) ==
    IF ~c.set THEN "null"
    ELSE Obj(<<KV("amount", Num(c.amount)), KV("type", Str(c.type)), KV("anchor", Num(c.anchor)),
               KV("next", Name(c.next)), KV("switchAt", OrNull(c.switchAt))>>)

RowJson(row) ==
    Obj(<<KV("end", Num(row.end)), KV("reported", Num(row.reported)),
          KV("unreported", Num(row.unreported)), KV("open", Num(row.open))>>)

ObjectJson(ob) ==
    LET held == SelectSeq(IdSeq, LAMBDA r : ob.holds[r].state # "none")
        starts == Sorted(DOMAIN ob.totals)
    IN Obj(<<KV("lastNow", Num(ob.lastNow)), KV("cap", CapJson(ob.cap)),
             KV("holds", Obj([i \in 1..Len(held) |-> KV("r" \o ToString(held[i]), HoldJson(ob.holds[held[i]]))])),
             KV("totals", Obj([i \in 1..Len(starts) |-> KV(ToString(starts[i]), RowJson(ob.totals[starts[i]]))])),
             KV("alarm", OrNull(ob.alarm))>>)

LineJson(m, done) ==
    Obj(<<KV("n", Num(w.n)), KV("i", Num(w.i)), KV("path", Str(w.path)), KV("active", Bool(w.active)),
          KV("k", Str(m.mv.k)), KV("own", Bool(m.mv.own)), KV("r", Id(m.mv.r)), KV("amt", Num(m.mv.amt)),
          KV("cost", Num(m.mv.cost)), KV("cap", Num(m.mv.cap)), KV("type", Name(m.mv.type)),
          KV("at", Num(done.clock)), KV("ans", AnswerJson(done.ans)), KV("s", ObjectJson(done.o)),
          KV("gw", Str(done.gw.pc))>>)

HeaderJson ==
    Obj(<<KV("header", "true"), KV("seed", Num(Seed)), KV("count", Num(Count)), KV("depth", Num(Depth)),
          KV("weekLen", Num(WeekLen)), KV("monthLen", Num(MonthLen)), KV("life", Num(Life)),
          KV("maxTime", Num(MaxTime)), KV("lastMonthStart", Num(LastMonthStart))>>)

ASSUME PrintT(HeaderJson)

Walk(n, x) == [n |-> n, i |-> 0, x |-> x, path |-> IF n % 2 = 1 THEN "host" ELSE "test",
               active |-> n % 7 # 0]

TraceInit ==
    /\ clock = 0
    /\ o = EmptyObject
    /\ gw = Idle
    /\ last = NoLast
    /\ hist = NoHistory
    /\ w = Walk(1, 1 + (Seed % 2147483646))

TraceNext ==
    /\ w.n <= Count
    /\ IF w.i = Depth
       THEN /\ clock' = 0
            /\ o' = EmptyObject
            /\ gw' = Idle
            /\ w' = Walk(w.n + 1, w.x)
       ELSE LET x1 == Lcg(w.x)
                x2 == Lcg(x1)
                m == Chosen(x1, x2)
                done == After(m)
            IN /\ PrintT(LineJson(m, done))
               /\ clock' = done.clock
               /\ o' = done.o
               /\ gw' = done.gw
               /\ w' = [w EXCEPT !.i = @ + 1, !.x = x2]
    /\ UNCHANGED <<last, hist>>
=============================================================================
