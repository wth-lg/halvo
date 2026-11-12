--------------------------- MODULE Confirm4Trace ---------------------------
EXTENDS Confirm4, TraceText

CONSTANTS Seed, Count, Depth

VARIABLE w

RefusePct == 12
KindSeq == <<"issue", "arm", "answer", "record", "release", "lapse", "stop", "guard", "other">>
RefusalSeq == <<"stale", "presenter", "early", "state", "approval">>

Weight(k) ==
    CASE k = "issue" -> 30
      [] k = "arm" -> 25
      [] k = "answer" -> 25
      [] k = "record" -> 25
      [] k = "release" -> 25
      [] k = "lapse" -> 4
      [] k = "stop" -> 2
      [] k = "guard" -> 12
      [] k = "other" -> 3

KindOf(mv) ==
    IF mv.m = "guard" /\ (mv.state = "committed" \/ mv.approval = NoApproval) THEN "other" ELSE mv.m
Fine(a) == a \in {"ok", "repeat"}

RECURSIVE WSum(_)
WSum(ks) == IF Len(ks) = 0 THEN 0 ELSE Weight(ks[1]) + WSum(Tail(ks))

RECURSIVE WPick(_, _)
WPick(ks, t) ==
    IF Len(ks) = 1 \/ t < Weight(ks[1]) THEN ks[1]
    ELSE WPick(Tail(ks), t - Weight(ks[1]))

I(ok) == [m |-> "issue", ok |-> ok]
A(i) == [m |-> "arm", item |-> i]
N(i, x, y) == [m |-> "answer", item |-> i, answer |-> x, from |-> y]
R(i) == [m |-> "record", item |-> i]
L(i) == [m |-> "release", item |-> i]
P(i, c, b) == [m |-> "lapse", item |-> i, cause |-> c, stored |-> b]
Z == [m |-> "stop"]
G(e, x, d, p) == [m |-> "guard", effect |-> e, state |-> x, digest |-> d, approval |-> p,
                  policy |-> IF x = "releasing" THEN "confirm" ELSE "none"]
GR(e, d, p) == [m |-> "guard", effect |-> e, state |-> "releasing", digest |-> d, approval |-> p,
                policy |-> "run"]

Allowed == <<I(TRUE), A(1), N(1, "allow", "native"), R(1), L(1)>>

Directed ==
    <<[case |-> "answer_twice",
       moves |-> <<I(TRUE), A(1), N(1, "allow", "native"), N(1, "reject", "native"), R(1),
                   N(1, "reject", "native"), L(1), N(1, "allow", "native")>>],
      [case |-> "answer_early", moves |-> <<I(TRUE), N(1, "allow", "native")>>],
      [case |-> "answer_page_native", moves |-> <<I(TRUE), A(1), N(1, "allow", "page")>>],
      [case |-> "timer_allow", moves |-> <<I(TRUE), A(1), P(1, "wait_limit", FALSE)>>],
      [case |-> "no_record",
       moves |-> <<I(TRUE), A(1), N(1, "allow", "native"), L(1), P(1, "wait_limit", FALSE),
                   P(1, "no_record", TRUE)>>],
      [case |-> "digest_shown", moves |-> <<I(FALSE)>>],
      [case |-> "reason_unknown_id", moves |-> Allowed \o <<G(1, "approved", 1, 0)>>],
      [case |-> "reason_used_row",
       moves |-> Allowed \o <<G(1, "approved", 1, 1), G(1, "approved", 1, 1), G(2, "approved", 1, 1),
                               G(1, "approved", 3, 1)>>],
      [case |-> "reason_digest", moves |-> Allowed \o <<G(1, "approved", 3, 1)>>],
      [case |-> "reason_effect", moves |-> Allowed \o <<G(2, "approved", 1, 1)>>],
      [case |-> "reason_reject_row",
       moves |-> <<I(TRUE), A(1), N(1, "reject", "native"), R(1), L(1), G(1, "approved", 1, 1)>>],
      [case |-> "reason_releasing_no_row",
       moves |-> Allowed \o <<GR(2, 2, NoApproval), G(2, "releasing", 2, 0)>>],
      [case |-> "reason_releasing_unused_row", moves |-> Allowed \o <<G(2, "releasing", 2, 2)>>],
      [case |-> "reason_stopped_run", moves |-> Allowed \o <<Z, G(1, "approved", 1, 1)>>],
      [case |-> "guard_releasing_other",
       moves |-> <<I(TRUE), A(1), N(1, "allow", "native"), R(1), G(1, "releasing", 1, 1), L(1),
                   G(1, "releasing", 1, NoApproval), GR(1, 1, NoApproval), GR(1, 1, 0),
                   G(1, "releasing", 1, 1), G(1, "releasing", 3, 1), G(1, "releasing", 1, 2)>>]>>

LapseCands(s, i) ==
    LET x == s.items[i].st
    IN IF x \in {"shown", "armed"}
       THEN <<P(i, "wait_limit", FALSE), P(i, "cancelled", FALSE),
              P(i, "run_ended", FALSE), P(i, "app_ended", FALSE)>>
       ELSE IF x = "answered"
       THEN <<P(i, "wait_limit", FALSE), P(i, "cancelled", FALSE),
              P(i, "no_record", FALSE), P(i, "no_record", TRUE),
              P(i, "run_ended", FALSE), P(i, "app_ended", FALSE)>>
       ELSE IF x = "recorded"
       THEN <<P(i, "run_ended", FALSE), P(i, "app_ended", FALSE), P(i, "wait_limit", FALSE)>>
       ELSE <<P(i, "wait_limit", FALSE)>>

ItemCands(s, i) ==
    <<A(i),
      N(i, "allow", "page"), N(i, "allow", "native"),
      N(i, "allow", "page"), N(i, "allow", "native"),
      N(i, "reject", "page"), N(i, "reject", "native"),
      R(i), L(i)>>
    \o LapseCands(s, i)

GuardCands(s) ==
    Cat([e \in 1..NEffects |->
            Cat([d \in 1..(NRequests * NEffects) |->
                    Cat([p \in 1..(s.nap + 1) |->
                            <<G(e, "approved", d, p - 1),
                              IF (e + d + p) % 2 = 0 THEN GR(e, d, p - 1) ELSE G(e, "releasing", d, p - 1)>>])])
            \o <<G(e, "committed", Digest(1, e), 0), G(e, "approved", Digest(1, e), NoApproval),
                 G(e, "releasing", Digest(1, e), NoApproval), GR(e, Digest(1, e), NoApproval)>>])

Cands(s) ==
    IF s.app = "ended" THEN <<>>
    ELSE IF s.run = "ended" /\ OpenItems(s) # {}
    THEN LET open == Sorted(OpenItems(s))
         IN [k \in 1..Len(open) |-> P(open[k], "run_ended", FALSE)]
    ELSE (IF s.run # "ended" /\ Len(s.items) < NRequests
          THEN LET Sent(mv) == Offered(s, h, mv)
               IN SelectSeq(<<I(TRUE), I(TRUE), I(TRUE), I(TRUE), I(TRUE), I(FALSE)>>, Sent)
          ELSE <<>>)
         \o Cat([i \in 1..Len(s.items) |-> ItemCands(s, i)])
         \o (IF s.run # "ended" THEN <<Z>> \o GuardCands(s) ELSE <<>>)

RECURSIVE AnswersOf(_, _, _)
AnswersOf(s, cs, i) ==
    IF i > Len(cs) THEN <<>> ELSE <<Step(s, cs[i]).r>> \o AnswersOf(s, cs, i + 1)

Choose(s, x0) ==
    LET cs == Cands(s)
        as == AnswersOf(s, cs, 1)
        IsGood(i) == Fine(as[i])
        IsBad(i) == ~Fine(as[i])
        idx == [i \in 1..Len(cs) |-> i]
        good == SelectSeq(idx, IsGood)
        bad == SelectSeq(idx, IsBad)
        x1 == Lcg(x0)
        x2 == Lcg(x1)
        x3 == Lcg(x2)
        refuse == Len(bad) > 0 /\ (Len(good) = 0 \/ Draw(x1, 100) < RefusePct)
        HasAnswer(a) == \E j \in 1..Len(bad) : as[bad[j]] = a
        answers == SelectSeq(RefusalSeq, HasAnswer)
        answer == answers[1 + Draw(x2, Len(answers))]
        WithAnswer(i) == as[i] = answer
        HasKind(k) == \E j \in 1..Len(good) : KindOf(cs[good[j]]) = k
        kinds == SelectSeq(KindSeq, HasKind)
        kind == WPick(kinds, Draw(x2, WSum(kinds)))
        OfKind(i) == KindOf(cs[i]) = kind
        pool == IF refuse THEN SelectSeq(bad, WithAnswer) ELSE SelectSeq(good, OfKind)
    IN [mv |-> cs[pool[1 + Draw(x3, Len(pool))]], x |-> x3,
        dead |-> \A j \in 1..Len(good) : KindOf(cs[good[j]]) = "other"]

YesNo(b) == IF b THEN Str("yes") ELSE Str("no")

EffectsJson(n) ==
    LET es == Sorted(ReqEffects(n))
    IN Arr([k \in 1..Len(es) |->
            Obj(<<KV("effect", Id("e", es[k])), KV("class", Str(Class(es[k]))),
                  KV("digest", Id("d", Digest(n, es[k])))>>)])

EntriesJson(entries) ==
    LET es == Sorted({e.effect : e \in entries})
    IN Arr([k \in 1..Len(es) |->
            LET e == CHOOSE x \in entries : x.effect = es[k]
            IN Obj(<<KV("effect", Id("e", e.effect)), KV("digest", Id("d", e.digest)),
                     KV("approval", Id("p", e.approval)), KV("answer", Str(e.answer))>>)])

SentJson(res) ==
    LET IsSent(e) == e.k = "sent"
        sent == SelectSeq(res.out, IsSent)
    IN IF Len(sent) = 0 THEN "[]" ELSE EntriesJson(sent[1].entries)

IdList(k, S) == LET xs == Sorted(S) IN Arr([i \in 1..Len(xs) |-> Id(k, xs[i])])

Snap(s) ==
    <<KV("items", Obj([i \in 1..Len(s.items) |-> KV("i" \o ToString(i), Str(s.items[i].st))])),
      KV("table", IdList("p", {t.row.approval : t \in s.table})),
      KV("used", IdList("p", {t.row.approval : t \in {u \in s.table : u.used}})),
      KV("run", Str(s.run))>>

Args(s, mv) ==
    CASE mv.m = "issue" ->
            <<KV("effects", EffectsJson(Len(s.items) + 1)), KV("digest_ok", YesNo(mv.ok))>>
      [] mv.m \in {"arm", "record", "release"} -> <<KV("item", Id("i", mv.item))>>
      [] mv.m = "answer" ->
            <<KV("item", Id("i", mv.item)), KV("answer", Str(mv.answer)), KV("from", Str(mv.from))>>
      [] mv.m = "lapse" ->
            <<KV("item", Id("i", mv.item)), KV("cause", Str(mv.cause)),
              KV("stored", IF mv.cause = "no_record" THEN YesNo(mv.stored) ELSE "null")>>
      [] mv.m = "stop" -> <<>>
      [] mv.m = "guard" ->
            <<KV("effect", Id("e", mv.effect)), KV("class", Str(Class(mv.effect))),
              KV("state", Str(mv.state)), KV("digest", Id("d", mv.digest)),
              KV("approval", IF mv.approval = NoApproval THEN "null" ELSE Id("p", mv.approval)),
              KV("policy", IF mv.policy = "none" THEN "null" ELSE Str(mv.policy))>>

Extra(s, mv, res) ==
    CASE mv.m = "issue" ->
            <<KV("item", IF Len(res.st.items) > Len(s.items) THEN Id("i", Len(res.st.items)) ELSE "null"),
              KV("sent", SentJson(res))>>
      [] mv.m = "answer" ->
            <<KV("approvals", IF res.r = "ok"
                              THEN IdList("p", {d.approval : d \in res.st.items[mv.item].ids})
                              ELSE "[]")>>
      [] mv.m \in {"release", "lapse"} -> <<KV("sent", SentJson(res))>>
      [] OTHER -> <<>>

RowsJson(name, item, entries) ==
    LET es == Sorted({e.effect : e \in entries})
    IN [k \in 1..Len(es) |->
            LET e == CHOOSE x \in entries : x.effect = es[k]
            IN Obj(<<KV("event", Str(name)), KV("item", Id("i", item)), KV("effect", Id("e", e.effect)),
                     KV("approval", Id("p", e.approval)), KV("digest", Id("d", e.digest)),
                     KV("answer", Str(e.answer))>>)]

EventsOf(x, mv) ==
    CASE x.k = "item_raised" -> <<Obj(<<KV("event", Str(x.k)), KV("item", Id("i", x.item))>>)>>
      [] x.k = "item_lapsed" ->
            <<Obj(<<KV("event", Str(x.k)), KV("item", Id("i", x.item)), KV("cause", Str(x.cause))>>)>>
      [] x.k = "item_answered" -> RowsJson(x.k, x.item, x.rows)
      [] x.k = "unacked_rows" ->
            IF mv.cause = "no_record" /\ mv.stored THEN RowsJson("item_answered", x.item, x.rows) ELSE <<>>
      [] x.k = "item_released" -> RowsJson(x.k, x.item, x.entries)
      [] OTHER -> <<>>

CallsOf(x) ==
    CASE x.k = "answer_confirm" ->
            <<Obj(<<KV("call", Str(x.k)), KV("item", Id("i", x.item)), KV("answer", Str(x.answer))>>)>>
      [] x.k = "cancel" -> <<Obj(<<KV("call", Str(x.k))>>)>>
      [] OTHER -> <<>>

MoveJson(s, mv, res) ==
    Obj(<<KV("m", Str(mv.m)), KV("a", Obj(Args(s, mv))), KV("r", Str(res.r)),
          KV("e", Arr(Cat([i \in 1..Len(res.out) |-> EventsOf(res.out[i], mv)]))),
          KV("p", Arr(Cat([i \in 1..Len(res.out) |-> CallsOf(res.out[i])]))),
          KV("s", Obj(Extra(s, mv, res) \o Snap(res.st)))>>)

Cases == Len(Directed)

TraceInit ==
    /\ st = Init0
    /\ h = InitH
    /\ w = [n |-> 1, k |-> 0, z |-> 0, x |-> 1 + (Seed % 2147483646), acc |-> <<>>]

Play(mv, x, dead) ==
    LET res == Step(st, mv)
    IN /\ st' = res.st
       /\ h' = Hist(h, mv, res)
       /\ w' = [w EXCEPT !.k = @ + 1, !.x = x, !.z = IF dead THEN @ + 1 ELSE 0,
                         !.acc = Append(@, MoveJson(st, mv, res))]

Emit(head) ==
    /\ PrintT(Obj(head \o <<KV("moves", Arr(w.acc))>>))
    /\ st' = Init0
    /\ h' = InitH
    /\ w' = [w EXCEPT !.n = @ + 1, !.k = 0, !.z = 0, !.acc = <<>>]

TraceNext ==
    /\ w.n <= Count + Cases
    /\ IF w.n <= Count
       THEN IF w.k = Depth \/ w.z >= 2 \/ Len(Cands(st)) = 0
            THEN Emit(<<KV("n", ToString(w.n))>>)
            ELSE LET c == Choose(st, w.x) IN Play(c.mv, c.x, c.dead)
       ELSE LET d == Directed[w.n - Count]
            IN IF w.k = Len(d.moves)
               THEN Emit(<<KV("n", ToString(w.n)), KV("case", Str(d.case))>>)
               ELSE Play(d.moves[w.k + 1], w.x, FALSE)
=============================================================================
