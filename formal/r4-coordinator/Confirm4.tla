------------------------------ MODULE Confirm4 ------------------------------
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS NEffects, Req1, Req2, Native

VARIABLES st, h

Effects == 1..NEffects
NRequests == 2
Items == 1..NRequests
Digests == 1..(NRequests * NEffects)
Final == {"released", "lapsed"}
Sources == {"page", "native"}
Answers == {"allow", "reject"}
RecStates == {"approved", "releasing", "committed"}
Causes == {"digest", "run_ended", "wait_limit", "cancelled", "no_record", "app_ended"}

ASSUME /\ Req1 \subseteq Effects
       /\ Req2 \subseteq Effects
       /\ Req1 # {}
       /\ Req2 # {}
       /\ Native \subseteq Effects

ReqEffects(n) == IF n = 1 THEN Req1 ELSE Req2
Digest(n, e) == (n - 1) * NEffects + e
Class(e) == IF e \in Native THEN "payment" ELSE "send"
Presenter(n) == IF ReqEffects(n) \cap Native # {} THEN "native" ELSE "page"
Once(ans) == IF ans = "allow" THEN "allow_once" ELSE "reject_once"

Init0 == [items |-> <<>>, nap |-> 0, table |-> {}, run |-> "live", app |-> "live"]

InitH ==
    [shown |-> [i \in Items |-> {}],
     moves |-> [i \in Items |-> <<>>],
     answers |-> {},
     minted |-> [i \in Items |-> {}],
     stored |-> {},
     acked |-> {},
     got |-> {},
     released |-> {},
     passed |-> {},
     stopped |-> FALSE]

OpenItems(s) == {i \in DOMAIN s.items : s.items[i].st \notin Final}

Mint(s, n) ==
    {[effect |-> e, approval |-> s.nap + Cardinality({x \in ReqEffects(n) : x <= e})] :
        e \in ReqEffects(n)}

Entries(n, ids, answer) ==
    {[effect |-> d.effect, digest |-> Digest(n, d.effect), approval |-> d.approval,
      answer |-> answer] : d \in ids}

ItemEntries(it) == Entries(it.req, it.ids, Once(it.ans))

Rows(i, entries) ==
    {[item |-> i, effect |-> e.effect, digest |-> e.digest, approval |-> e.approval,
      answer |-> e.answer] : e \in entries}

Mv(i, m, cause) == [k |-> "move", item |-> i, m |-> m, cause |-> cause]
No(s, r) == [r |-> r, st |-> s, ev |-> <<>>]

Issue(s, ok) ==
    LET n == Len(s.items) + 1
        it == [req |-> n, st |-> "shown", ans |-> "none", ids |-> {}]
        shown == {[effect |-> e, digest |-> IF ok THEN Digest(n, e) ELSE 0] : e \in ReqEffects(n)}
    IN IF s.run # "live" THEN No(s, "state")
       ELSE IF OpenItems(s) # {} THEN No([s EXCEPT !.run = "ended"], "state")
       ELSE IF ok
       THEN [r |-> "ok", st |-> [s EXCEPT !.items = Append(@, it)],
             ev |-> <<[k |-> "shown", item |-> n, shown |-> shown], Mv(n, "issue", "none")>>]
       ELSE [r |-> "ok",
             st |-> [s EXCEPT !.items = Append(@, [it EXCEPT !.st = "lapsed"]),
                              !.nap = @ + Cardinality(ReqEffects(n))],
             ev |-> <<Mv(n, "issue", "none"),
                      [k |-> "sent", item |-> n, via |-> "lapse",
                       entries |-> Entries(n, Mint(s, n), "reject_once")],
                      Mv(n, "lapse", "digest")>>]

Arm(s, i) ==
    IF i \notin DOMAIN s.items THEN No(s, "state")
    ELSE IF s.items[i].st # "shown" THEN No(s, "state")
    ELSE [r |-> "ok", st |-> [s EXCEPT !.items[i].st = "armed"], ev |-> <<Mv(i, "arm", "none")>>]

Answer(s, i, ans, from) ==
    IF i \notin DOMAIN s.items THEN No(s, "state")
    ELSE LET it == s.items[i]
             ids == Mint(s, it.req)
         IN IF it.st \in {"answered", "recorded", "released", "lapsed"} THEN No(s, "stale")
            ELSE IF from # Presenter(it.req) THEN No(s, "presenter")
            ELSE IF it.st = "shown" THEN No(s, "early")
            ELSE [r |-> "ok",
                  st |-> [s EXCEPT !.items[i] = [it EXCEPT !.st = "answered", !.ans = ans, !.ids = ids],
                                   !.nap = @ + Cardinality(ReqEffects(it.req))],
                  ev |-> <<[k |-> "answer", item |-> i, answer |-> ans, from |-> from,
                            ids |-> {d.approval : d \in ids}],
                           Mv(i, "answer", "none")>>]

Record(s, i) ==
    IF i \notin DOMAIN s.items THEN No(s, "state")
    ELSE IF s.items[i].st # "answered" THEN No(s, "state")
    ELSE LET rows == Rows(i, ItemEntries(s.items[i]))
         IN [r |-> "ok",
             st |-> [s EXCEPT !.items[i].st = "recorded",
                              !.table = @ \cup {[row |-> x, live |-> FALSE, used |-> FALSE] : x \in rows}],
             ev |-> <<[k |-> "rows", rows |-> rows, acked |-> TRUE], Mv(i, "record", "none")>>]

Release(s, i) ==
    IF i \notin DOMAIN s.items THEN No(s, "state")
    ELSE IF s.items[i].st # "recorded" THEN No(s, "state")
    ELSE LET es == ItemEntries(s.items[i])
         IN [r |-> "ok",
             st |-> [s EXCEPT !.items[i].st = "released",
                              !.table = {IF t.row.item = i THEN [t EXCEPT !.live = TRUE] ELSE t : t \in @}],
             ev |-> <<[k |-> "sent", item |-> i, via |-> "release", entries |-> es],
                      Mv(i, "release", "none")>>]

LapseOk(it, cause) ==
    \/ it.st \in {"shown", "armed", "answered"}
    \/ it.st = "recorded" /\ cause \in {"run_ended", "app_ended"}

Lapse(s, i, cause, stored) ==
    IF i \notin DOMAIN s.items THEN No(s, "state")
    ELSE IF ~LapseOk(s.items[i], cause) THEN No(s, "state")
    ELSE LET it == s.items[i]
             gone == cause \in {"run_ended", "app_ended"}
             kept == cause = "no_record" /\ stored /\ it.st = "answered"
             es == Entries(it.req, Mint(s, it.req), "reject_once")
         IN [r |-> "ok",
             st |-> [s EXCEPT !.items[i].st = "lapsed",
                              !.nap = IF gone THEN @ ELSE @ + Cardinality(ReqEffects(it.req)),
                              !.table = {t \in @ : t.row.item # i},
                              !.run = IF gone THEN "ended" ELSE @,
                              !.app = IF cause = "app_ended" THEN "ended" ELSE @],
             ev |-> (IF kept THEN <<[k |-> "rows", rows |-> Rows(i, ItemEntries(it)), acked |-> FALSE]>> ELSE <<>>)
                    \o (IF gone THEN <<>> ELSE <<[k |-> "sent", item |-> i, via |-> "lapse", entries |-> es]>>)
                    \o <<Mv(i, "lapse", cause)>>]

Guard(s, rec) ==
    LET pass == [r |-> "ok", st |-> s, ev |-> <<[k |-> "pass", rec |-> rec]>>]
        m == {t \in s.table : /\ t.row.approval = rec.approval
                               /\ t.row.effect = rec.effect
                               /\ t.row.digest = rec.digest
                               /\ t.row.answer = "allow_once"
                               /\ t.live}
    IN IF rec.state \notin {"approved", "releasing"} THEN No(s, "ok")
       ELSE IF rec.state = "approved"
       THEN IF {t \in m : ~t.used} = {} THEN No(s, "approval")
            ELSE [pass EXCEPT !.st = [s EXCEPT !.table = (@ \ m) \cup {[t EXCEPT !.used = TRUE] : t \in m}]]
       ELSE IF rec.effect \in Native
       THEN IF {t \in m : t.used} = {} THEN No(s, "approval") ELSE pass
       ELSE No(s, "ok")

Step(s, mv) ==
    CASE mv.m = "issue" -> Issue(s, mv.ok)
      [] mv.m = "arm" -> Arm(s, mv.item)
      [] mv.m = "answer" -> Answer(s, mv.item, mv.answer, mv.from)
      [] mv.m = "record" -> Record(s, mv.item)
      [] mv.m = "release" -> Release(s, mv.item)
      [] mv.m = "lapse" -> Lapse(s, mv.item, mv.cause, mv.stored)
      [] mv.m = "guard" -> Guard(s, mv)

ItemMoves(s, i) ==
    {[m |-> "arm", item |-> i], [m |-> "record", item |-> i], [m |-> "release", item |-> i]}
    \cup {[m |-> "answer", item |-> i, answer |-> x, from |-> y] : x \in Answers, y \in Sources}
    \cup {[m |-> "lapse", item |-> i, cause |-> c, stored |-> FALSE] : c \in Causes}
    \cup {[m |-> "lapse", item |-> i, cause |-> "no_record", stored |-> TRUE]}

GuardMoves(s) ==
    UNION {{[m |-> "guard", effect |-> e, state |-> x, digest |-> d, approval |-> p] :
                x \in RecStates, d \in {Digest(1, e), Digest(2, e)}, p \in 0..s.nap} :
           e \in Effects}

Moves(s) ==
    IF s.app = "ended" THEN {}
    ELSE IF s.run = "ended"
    THEN IF OpenItems(s) # {}
         THEN {[m |-> "lapse", item |-> i, cause |-> "run_ended", stored |-> FALSE] : i \in OpenItems(s)}
         ELSE UNION {ItemMoves(s, i) : i \in DOMAIN s.items}
    ELSE (IF Len(s.items) < NRequests THEN {[m |-> "issue", ok |-> b] : b \in BOOLEAN} ELSE {})
         \cup UNION {ItemMoves(s, i) : i \in DOMAIN s.items}
         \cup GuardMoves(s)

HLast(g, i) == IF Len(g.moves[i]) = 0 THEN "none" ELSE g.moves[i][Len(g.moves[i])].m

HEvent(g, e) ==
    CASE e.k = "shown" -> [g EXCEPT !.shown[e.item] = e.shown]
      [] e.k = "move" -> [g EXCEPT !.moves[e.item] = Append(@, [m |-> e.m, cause |-> e.cause])]
      [] e.k = "answer" ->
            [g EXCEPT !.answers = @ \cup {[item |-> e.item, answer |-> e.answer, from |-> e.from,
                                           armed |-> HLast(g, e.item) = "arm"]},
                      !.minted[e.item] = @ \cup e.ids]
      [] e.k = "rows" ->
            [g EXCEPT !.stored = @ \cup e.rows,
                      !.acked = IF e.acked THEN @ \cup e.rows ELSE @]
      [] e.k = "sent" ->
            [g EXCEPT !.got = @ \cup {[item |-> e.item, via |-> e.via, entries |-> e.entries,
                                       after |-> g.stopped]}]
      [] e.k = "released" -> [g EXCEPT !.released = @ \cup {[item |-> e.item, entries |-> e.entries]}]
      [] e.k = "pass" ->
            [g EXCEPT !.passed = @ \cup
                {[state |-> e.rec.state, effect |-> e.rec.effect,
                  digest |-> IF e.rec.state = "releasing" /\ e.rec.effect \notin Native THEN 0 ELSE e.rec.digest,
                  approval |-> e.rec.approval,
                  first |-> \/ e.rec.state = "releasing"
                            \/ \A p \in g.passed : ~(p.state = "approved" /\ p.approval = e.rec.approval),
                  used |-> \E p \in g.passed : /\ p.state = "approved"
                                               /\ p.effect = e.rec.effect
                                               /\ p.digest = e.rec.digest
                                               /\ p.approval = e.rec.approval,
                  stopped |-> g.stopped]}]
      [] e.k = "stop" -> [g EXCEPT !.stopped = TRUE]

RECURSIVE HFold(_, _, _)
HFold(g, evs, i) == IF i > Len(evs) THEN g ELSE HFold(HEvent(g, evs[i]), evs, i + 1)

Hist(g, res) == HFold(g, res.ev, 1)

Init == st = Init0 /\ h = InitH
Next == \E mv \in Moves(st) : LET res == Step(st, mv) IN st' = res.st /\ h' = Hist(h, res)

Entry(x) == [effect |-> x.effect, digest |-> x.digest, approval |-> x.approval, answer |-> x.answer]
Released(x) == \E d \in h.got : d.via = "release" /\ d.item = x.item /\ Entry(x) \in d.entries
Row(p) == {x \in h.acked : /\ x.approval = p.approval
                            /\ x.effect = p.effect
                            /\ x.digest = p.digest
                            /\ x.answer = "allow_once"
                            /\ Released(x)}
Lapsed(i) == \E k \in 1..Len(h.moves[i]) : h.moves[i][k].m = "lapse"

L1 == \A d \in h.got : \A e \in d.entries : e.answer = "allow_once" =>
        \E a \in h.answers : /\ a.item = d.item
                             /\ a.answer = "allow"
                             /\ a.armed
                             /\ a.from = Presenter(d.item)
L2 == \A d \in h.got : d.via = "release" =>
        {[effect |-> e.effect, digest |-> e.digest] : e \in d.entries} = h.shown[d.item]
L3 == /\ \A c, d \in h.got : \A x \in c.entries : \A y \in d.entries :
            x.approval = y.approval => (c = d /\ x = y)
      /\ \A x, y \in h.stored : x.approval = y.approval => x = y
      /\ \A x \in h.acked : \A d \in h.got :
            (d.via = "release" /\ d.item = x.item) => \E e \in d.entries : e.approval = x.approval
      /\ \A d \in h.got : d.via = "lapse" =>
            \A e \in d.entries : /\ e.approval \notin h.minted[d.item]
                                 /\ \A x \in h.stored : x.approval # e.approval
L4 == \A i \in Items : \A k \in 1..Len(h.moves[i]) :
        h.moves[i][k].m \in {"release", "lapse"} => k = Len(h.moves[i])

L5 == \A d \in h.got : d.via = "release" =>
        \A e \in d.entries : \E x \in h.acked : x.approval = e.approval

L6 == \A d \in h.got : d.via = "lapse" => \A e \in d.entries : e.answer = "reject_once"

L7 == \A p \in h.passed : p.state = "approved" =>
        /\ p.first
        /\ \E x \in h.acked : /\ x.approval = p.approval
                              /\ x.effect = p.effect
                              /\ x.digest = p.digest
                              /\ x.answer = "allow_once"

ReachEffects == \A d \in h.got : ~(/\ d.via = "release"
                                   /\ Cardinality(d.entries) >= 2
                                   /\ \A e \in d.entries : e.answer = "allow_once")
ReachSecond == ~(/\ Len(st.items) = 2
                 /\ \A i \in Items : \E d \in h.got : /\ d.via = "release"
                                                      /\ d.item = i
                                                      /\ \A e \in d.entries : e.answer = "allow_once"
                 /\ \E e \in Req1 \cap Req2 : Digest(1, e) # Digest(2, e))
ReachPresenters == ~({a.from : a \in h.answers} = Sources)
ReachLostAck == h.stored = h.acked
ReachRestart == ~(st.app = "ended" /\ \E i \in Items : \E x \in h.acked : x.item = i /\ Lapsed(i))
ReachReleasedRestart == ~(/\ st.app = "ended"
                          /\ \E x \in h.released : x.item = 1
                          /\ \E x \in h.acked : x.item = 2
                          /\ \A x \in h.released : x.item # 2)
ReachLapseRecorded == \A i \in Items : \A k \in 1..Len(h.moves[i]) :
                        ~(h.moves[i][k].m = "lapse" /\ \E j \in 1..(k - 1) : h.moves[i][j].m = "record")
ReachOldApproval ==
    ~(/\ Len(st.items) = 2
      /\ \E t, u \in st.table :
            /\ t.row.item = 1 /\ u.row.item = 2 /\ t.row.effect = u.row.effect
            /\ t.live /\ u.live
            /\ t.row.answer = "allow_once" /\ u.row.answer = "allow_once"
            /\ Guard(st, [state |-> "approved", effect |-> t.row.effect, digest |-> u.row.digest,
                          approval |-> t.row.approval]).r = "approval"
            /\ Guard(st, [state |-> "approved", effect |-> u.row.effect, digest |-> u.row.digest,
                          approval |-> u.row.approval]).r = "ok")
ReachMoney == \A p \in h.passed : ~(p.state = "releasing" /\ p.effect \in Native)
ReachOtherEffect ==
    ~(\E t \in st.table : /\ t.live /\ ~t.used /\ t.row.answer = "allow_once"
                          /\ \E e \in Effects \ {t.row.effect} :
                                Guard(st, [state |-> "approved", effect |-> e, digest |-> t.row.digest,
                                           approval |-> t.row.approval]).r = "approval")
ReachStopLapse == ~(st.run = "stopped" /\ \E i \in Items : Lapsed(i))
ReachStopPerson == ~(st.run = "stopped" /\ \A i \in Items : ~Lapsed(i))
ReachStoppedRequest == ~(st.run = "stopped" /\ Len(st.items) < NRequests /\ Issue(st, TRUE).r = "state")
ReachStoppedRecord ==
    ~(/\ st.run = "stopped"
      /\ \E t \in st.table : /\ t.live /\ ~t.used /\ t.row.answer = "allow_once"
                             /\ Guard(st, [state |-> "approved", effect |-> t.row.effect,
                                           digest |-> t.row.digest, approval |-> t.row.approval]).r = "approval")
ReachRejected == \A d \in h.got : d.via # "release" \/ \A e \in d.entries : e.answer = "allow_once"

EveryIdHasRow == \A d \in h.got : \A e \in d.entries : \E x \in h.stored : x.approval = e.approval
RowsAreSent == \A x \in h.stored : x.answer = "allow_once" =>
                    \/ \E i \in OpenItems(st) : i = x.item
                    \/ \E d \in h.got : d.item = x.item /\ d.via = "release"
=============================================================================
