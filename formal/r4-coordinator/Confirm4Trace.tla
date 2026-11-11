--------------------------- MODULE Confirm4Trace ---------------------------
EXTENDS Confirm4, TraceText

CONSTANTS Seed, Count, Depth

VARIABLE w

RefusePct == 12
KindSeq == <<"issue", "arm", "answer", "record", "release", "lapse", "guard", "other">>
RefusalSeq == <<"stale", "presenter", "early", "state", "approval">>

Weight(k) ==
    CASE k = "issue" -> 30
      [] k = "arm" -> 25
      [] k = "answer" -> 25
      [] k = "record" -> 25
      [] k = "release" -> 25
      [] k = "lapse" -> 4
      [] k = "guard" -> 12
      [] k = "other" -> 3

KindOf(mv) == IF mv.m = "guard" /\ mv.state # "approved" THEN "other" ELSE mv.m

RECURSIVE WSum(_)
WSum(ks) == IF Len(ks) = 0 THEN 0 ELSE Weight(ks[1]) + WSum(Tail(ks))

RECURSIVE WPick(_, _)
WPick(ks, t) ==
    IF Len(ks) = 1 \/ t < Weight(ks[1]) THEN ks[1]
    ELSE WPick(Tail(ks), t - Weight(ks[1]))

LapseCand(i, c, b) == [m |-> "lapse", item |-> i, cause |-> c, stored |-> b]

LapseCands(s, i) ==
    LET x == s.items[i].st
    IN IF x \in {"shown", "armed"}
       THEN <<LapseCand(i, "wait_limit", FALSE), LapseCand(i, "cancelled", FALSE),
              LapseCand(i, "run_ended", FALSE), LapseCand(i, "app_ended", FALSE)>>
       ELSE IF x = "answered"
       THEN <<LapseCand(i, "wait_limit", FALSE), LapseCand(i, "cancelled", FALSE),
              LapseCand(i, "no_record", FALSE), LapseCand(i, "no_record", TRUE),
              LapseCand(i, "run_ended", FALSE), LapseCand(i, "app_ended", FALSE)>>
       ELSE IF x = "recorded"
       THEN <<LapseCand(i, "run_ended", FALSE), LapseCand(i, "app_ended", FALSE),
              LapseCand(i, "wait_limit", FALSE)>>
       ELSE <<LapseCand(i, "wait_limit", FALSE)>>

ItemCands(s, i) ==
    <<[m |-> "arm", item |-> i],
      [m |-> "answer", item |-> i, answer |-> "allow", from |-> "page"],
      [m |-> "answer", item |-> i, answer |-> "allow", from |-> "native"],
      [m |-> "answer", item |-> i, answer |-> "allow", from |-> "page"],
      [m |-> "answer", item |-> i, answer |-> "allow", from |-> "native"],
      [m |-> "answer", item |-> i, answer |-> "reject", from |-> "page"],
      [m |-> "answer", item |-> i, answer |-> "reject", from |-> "native"],
      [m |-> "record", item |-> i],
      [m |-> "release", item |-> i]>>
    \o LapseCands(s, i)

GuardCand(e, x, d, p) == [m |-> "guard", effect |-> e, state |-> x, digest |-> d, approval |-> p]

GuardCands(s) ==
    Cat([e \in 1..NEffects |->
            Cat([d \in 1..2 |-> [p \in 1..(s.nap + 1) |->
                    GuardCand(e, "approved", Digest(d, e), p - 1)]])
            \o (IF e \in Native
                THEN Cat([d \in 1..2 |-> [p \in 1..(s.nap + 1) |->
                        GuardCand(e, "releasing", Digest(d, e), p - 1)]])
                ELSE <<GuardCand(e, "releasing", Digest(1, e), 0)>>)
            \o <<GuardCand(e, "committed", Digest(1, e), 0)>>])

Cands(s) ==
    IF s.app = "ended" THEN <<>>
    ELSE IF s.run = "ended" /\ OpenItems(s) # {}
    THEN LET open == Sorted(OpenItems(s))
         IN [k \in 1..Len(open) |-> LapseCand(open[k], "run_ended", FALSE)]
    ELSE (IF s.run = "live" /\ Len(s.items) < NRequests
          THEN <<[m |-> "issue", ok |-> TRUE], [m |-> "issue", ok |-> TRUE],
                 [m |-> "issue", ok |-> TRUE], [m |-> "issue", ok |-> TRUE],
                 [m |-> "issue", ok |-> TRUE], [m |-> "issue", ok |-> FALSE]>>
          ELSE <<>>)
         \o Cat([i \in 1..Len(s.items) |-> ItemCands(s, i)])
         \o (IF s.run = "live" THEN GuardCands(s) ELSE <<>>)

RECURSIVE AnswersOf(_, _, _)
AnswersOf(s, cs, i) ==
    IF i > Len(cs) THEN <<>> ELSE <<Step(s, cs[i]).r>> \o AnswersOf(s, cs, i + 1)

Choose(s, x0) ==
    LET cs == Cands(s)
        as == AnswersOf(s, cs, 1)
        IsGood(i) == as[i] = "ok"
        IsBad(i) == as[i] # "ok"
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
        mv == cs[pool[1 + Draw(x3, Len(pool))]]
    IN [mv |-> mv, res |-> Step(s, mv), x |-> x3,
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

SentJson(s, t) ==
    LET new == t.sent \ s.sent
    IN IF new = {} THEN "[]" ELSE EntriesJson((CHOOSE d \in new : TRUE).entries)

IdList(k, S) == LET xs == Sorted(S) IN Arr([i \in 1..Len(xs) |-> Id(k, xs[i])])

Snap(s) ==
    <<KV("items", Obj([i \in 1..Len(s.items) |-> KV("i" \o ToString(i), Str(s.items[i].st))])),
      KV("table", IdList("p", {t.approval : t \in s.table})),
      KV("used", IdList("p", {t.approval : t \in {u \in s.table : u.used}})),
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
      [] mv.m = "guard" ->
            <<KV("effect", Id("e", mv.effect)), KV("class", Str(Class(mv.effect))),
              KV("state", Str(mv.state)), KV("digest", Id("d", mv.digest)),
              KV("approval", Id("p", mv.approval))>>

Extra(s, mv, res) ==
    CASE mv.m = "issue" ->
            <<KV("item", IF Len(res.st.items) > Len(s.items) THEN Id("i", Len(res.st.items)) ELSE "null"),
              KV("sent", SentJson(s, res.st))>>
      [] mv.m = "answer" ->
            <<KV("approvals", IF res.r = "ok"
                              THEN IdList("p", {d.approval : d \in res.st.items[mv.item].dec})
                              ELSE "[]")>>
      [] mv.m \in {"release", "lapse"} -> <<KV("sent", SentJson(s, res.st))>>
      [] OTHER -> <<>>

MoveJson(s, mv, res) ==
    Obj(<<KV("m", Str(mv.m)), KV("a", Obj(Args(s, mv))), KV("r", Str(res.r)),
          KV("s", Obj(Extra(s, mv, res) \o Snap(res.st)))>>)

TraceInit ==
    /\ st = Init0
    /\ h = InitH
    /\ w = [n |-> 1, k |-> 0, z |-> 0, x |-> 1 + (Seed % 2147483646), acc |-> <<>>]

TraceNext ==
    /\ w.n <= Count
    /\ IF w.k = Depth \/ w.z >= 2 \/ Len(Cands(st)) = 0
       THEN /\ PrintT(Obj(<<KV("n", ToString(w.n)), KV("moves", Arr(w.acc))>>))
            /\ st' = Init0
            /\ h' = InitH
            /\ w' = [w EXCEPT !.n = @ + 1, !.k = 0, !.z = 0, !.acc = <<>>]
       ELSE LET c == Choose(st, w.x)
            IN /\ st' = c.res.st
               /\ h' = Hist(h, c.res)
               /\ w' = [w EXCEPT !.k = @ + 1, !.x = c.x,
                                 !.z = IF c.dead THEN @ + 1 ELSE 0,
                                 !.acc = Append(@, MoveJson(st, c.mv, c.res))]
=============================================================================
