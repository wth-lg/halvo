------------------------------ MODULE Confirm4 ------------------------------
EXTENDS Integers, Sequences, FiniteSets

CONSTANTS NEffects, Req1, Req2, Native

VARIABLE st

Effects == 1..NEffects
NRequests == 2
Final == {"released", "lapsed"}
Sources == {"page", "native"}
Answers == {"allow", "reject"}
RecStates == {"approved", "releasing", "committed"}

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

Init0 ==
    [items |-> <<>>, nap |-> 0, rows |-> {}, table |-> {}, sent |-> {}, passed |-> <<>>,
     run |-> "live", app |-> "live"]

OpenItems(s) == {i \in DOMAIN s.items : s.items[i].st \notin Final}

Mint(s, n) ==
    {[effect |-> e, approval |-> s.nap + Cardinality({x \in ReqEffects(n) : x <= e})] :
        e \in ReqEffects(n)}

Entries(it, answer) ==
    {[effect |-> d.effect, digest |-> Digest(it.req, d.effect), approval |-> d.approval,
      answer |-> answer] : d \in it.dec}

Rows(i, it) ==
    {[item |-> i, effect |-> e.effect, digest |-> e.digest, approval |-> e.approval,
      answer |-> e.answer] : e \in Entries(it, Once(it.ans))}

Issue(s, ok) ==
    LET n == Len(s.items) + 1
        it == [req |-> n, st |-> "shown", ans |-> "none", by |-> "none", was |-> "none",
               cause |-> "none", dec |-> {}, final |-> "none",
               shown |-> {[effect |-> e, digest |-> IF ok THEN Digest(n, e) ELSE 0] : e \in ReqEffects(n)}]
        no == [it EXCEPT !.st = "lapsed", !.cause = "digest", !.dec = Mint(s, n),
                         !.final = "lapsed", !.shown = {}]
    IN IF OpenItems(s) # {} THEN [r |-> "state", st |-> [s EXCEPT !.run = "ended"]]
       ELSE IF ok THEN [r |-> "ok", st |-> [s EXCEPT !.items = Append(@, it)]]
       ELSE [r |-> "ok",
             st |-> [s EXCEPT !.items = Append(@, no),
                              !.nap = @ + Cardinality(ReqEffects(n)),
                              !.sent = @ \cup {[item |-> n, via |-> "lapse", acked |-> FALSE,
                                                entries |-> Entries(no, "reject_once")]}]]

Arm(s, i) ==
    IF i \notin DOMAIN s.items THEN [r |-> "state", st |-> s]
    ELSE IF s.items[i].st # "shown" THEN [r |-> "state", st |-> s]
    ELSE [r |-> "ok", st |-> [s EXCEPT !.items[i].st = "armed"]]

Answer(s, i, ans, from) ==
    IF i \notin DOMAIN s.items THEN [r |-> "state", st |-> s]
    ELSE LET it == s.items[i]
         IN IF it.st \in {"answered", "recorded", "released", "lapsed"} THEN [r |-> "stale", st |-> s]
            ELSE IF from # Presenter(it.req) THEN [r |-> "presenter", st |-> s]
            ELSE IF it.st = "shown" THEN [r |-> "early", st |-> s]
            ELSE [r |-> "ok",
                  st |-> [s EXCEPT !.items[i] = [it EXCEPT !.st = "answered", !.ans = ans,
                                                           !.by = from, !.was = it.st,
                                                           !.dec = Mint(s, it.req)],
                                   !.nap = @ + Cardinality(ReqEffects(it.req))]]

Record(s, i) ==
    IF i \notin DOMAIN s.items THEN [r |-> "state", st |-> s]
    ELSE IF s.items[i].st # "answered" THEN [r |-> "state", st |-> s]
    ELSE [r |-> "ok",
          st |-> [s EXCEPT !.items[i].st = "recorded",
                           !.rows = @ \cup Rows(i, s.items[i]),
                           !.table = @ \cup {[item |-> x.item, effect |-> x.effect,
                                              digest |-> x.digest, approval |-> x.approval,
                                              answer |-> x.answer, used |-> FALSE] :
                                                x \in Rows(i, s.items[i])}]]

Release(s, i) ==
    IF i \notin DOMAIN s.items THEN [r |-> "state", st |-> s]
    ELSE IF s.items[i].st # "recorded" THEN [r |-> "state", st |-> s]
    ELSE LET es == Entries(s.items[i], Once(s.items[i].ans))
         IN [r |-> "ok",
             st |-> [s EXCEPT !.items[i].st = "released",
                              !.items[i].final = IF @ = "none" THEN "released" ELSE @,
                              !.sent = @ \cup {[item |-> i, via |-> "release", entries |-> es,
                                                acked |-> \A e \in es : \E t \in s.table :
                                                            t.approval = e.approval]}]]

LapseOk(it, cause) ==
    \/ it.st \in {"shown", "armed", "answered"}
    \/ it.st = "recorded" /\ cause \in {"run_ended", "app_ended"}

Lapse(s, i, cause, stored) ==
    IF i \notin DOMAIN s.items THEN [r |-> "state", st |-> s]
    ELSE IF ~LapseOk(s.items[i], cause) THEN [r |-> "state", st |-> s]
    ELSE LET it == s.items[i]
             gone == cause \in {"run_ended", "app_ended"}
             fresh == ~gone /\ it.dec = {}
             it1 == [it EXCEPT !.st = "lapsed", !.cause = cause,
                               !.dec = IF fresh THEN Mint(s, it.req) ELSE @,
                               !.final = IF @ = "none" THEN "lapsed" ELSE @]
         IN [r |-> "ok",
             st |-> [s EXCEPT !.items[i] = it1,
                              !.nap = IF fresh THEN @ + Cardinality(ReqEffects(it.req)) ELSE @,
                              !.run = IF gone THEN "ended" ELSE @,
                              !.app = IF cause = "app_ended" THEN "ended" ELSE @,
                              !.rows = IF cause = "no_record" /\ stored THEN @ \cup Rows(i, it) ELSE @,
                              !.sent = IF gone THEN @
                                       ELSE @ \cup {[item |-> i, via |-> "lapse", acked |-> FALSE,
                                                     entries |-> Entries(it1, "reject_once")]}]]

Match(s, rec, used) ==
    {x \in s.table : /\ x.effect = rec.effect
                     /\ x.digest = rec.digest
                     /\ x.approval = rec.approval
                     /\ x.answer = "allow_once"
                     /\ x.used = used}

Good(s, rec) ==
    \E t \in s.table : /\ t.effect = rec.effect
                       /\ t.digest = rec.digest
                       /\ t.approval = rec.approval
                       /\ t.answer = "allow_once"
                       /\ ~t.used

Released(s, rec) ==
    \E d \in s.sent : /\ d.via = "release"
                      /\ \E e \in d.entries : e.approval = rec.approval /\ e.answer = "allow_once"

Guard(s, rec) ==
    IF rec.state = "approved"
    THEN LET m == Match(s, rec, FALSE)
         IN IF m = {} THEN [r |-> "approval", st |-> s]
            ELSE [r |-> "ok",
                  st |-> [s EXCEPT !.table = (@ \ m) \cup {[t EXCEPT !.used = TRUE] : t \in m},
                                   !.passed = Append(@, [approval |-> rec.approval,
                                                         good |-> Good(s, rec),
                                                         rel |-> Released(s, rec)])]]
    ELSE IF rec.state = "releasing" /\ rec.effect \in Native
    THEN IF Match(s, rec, TRUE) = {} THEN [r |-> "approval", st |-> s] ELSE [r |-> "ok", st |-> s]
    ELSE [r |-> "ok", st |-> s]

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
    \cup {[m |-> "lapse", item |-> i, cause |-> c, stored |-> FALSE] :
            c \in {"wait_limit", "cancelled", "no_record", "run_ended", "app_ended"}}
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

Init == st = Init0
Next == \E mv \in Moves(st) : st' = Step(st, mv).st

Provenance(i) ==
    LET it == st.items[i]
    IN it.ans = "allow" /\ it.was = "armed" /\ it.by = Presenter(it.req)

L1 == /\ \A d \in st.sent : \A e \in d.entries : e.answer = "allow_once" => Provenance(d.item)
      /\ \A x \in st.rows : x.answer = "allow_once" => Provenance(x.item)
L2 == \A d \in st.sent : d.via = "release" =>
        \A e \in d.entries : [effect |-> e.effect, digest |-> e.digest] \in st.items[d.item].shown
L3 == /\ \A i, j \in DOMAIN st.items : \A x \in st.items[i].dec : \A y \in st.items[j].dec :
            x.approval = y.approval => (i = j /\ x = y)
      /\ \A x, y \in st.rows : x.approval = y.approval => x = y
      /\ \A x \in st.rows : [effect |-> x.effect, approval |-> x.approval] \in st.items[x.item].dec
      /\ \A c, d \in st.sent : c.item = d.item => c = d
L4 == \A i \in DOMAIN st.items : st.items[i].final # "none" => st.items[i].st = st.items[i].final
L5 == \A d \in st.sent : d.via = "release" => d.acked
L6 == \A d \in st.sent : d.via = "lapse" => \A e \in d.entries : e.answer = "reject_once"
L7 == /\ \A k \in DOMAIN st.passed : st.passed[k].good
      /\ \A j, k \in DOMAIN st.passed : st.passed[j].approval = st.passed[k].approval => j = k

ReachEffects == \A d \in st.sent : ~(/\ d.via = "release"
                                     /\ Cardinality(d.entries) >= 2
                                     /\ \A e \in d.entries : e.answer = "allow_once")
ReachSecond == ~(/\ Len(st.items) = 2
                 /\ st.items[1].st = "released" /\ st.items[1].ans = "allow"
                 /\ st.items[2].st = "released" /\ st.items[2].ans = "allow"
                 /\ \E e \in Req1 \cap Req2 : Digest(1, e) # Digest(2, e))
ReachPresenters == ~(/\ Len(st.items) = 2
                     /\ {st.items[1].by, st.items[2].by} = Sources)
ReachLostAck == \A i \in DOMAIN st.items :
                    ~(st.items[i].cause = "no_record" /\ \E x \in st.rows : x.item = i)
ReachRestart == \A i \in DOMAIN st.items :
                    ~(/\ st.items[i].cause = "app_ended"
                      /\ st.items[i].ans = "allow"
                      /\ \E t \in st.table : t.item = i)
ReachOldApproval ==
    ~(/\ Len(st.items) = 2
      /\ \E t, u \in st.table :
            /\ t.item = 1 /\ u.item = 2 /\ t.effect = u.effect
            /\ t.answer = "allow_once" /\ u.answer = "allow_once"
            /\ Guard(st, [state |-> "approved", effect |-> t.effect, digest |-> u.digest,
                          approval |-> t.approval]).r = "approval"
            /\ Guard(st, [state |-> "approved", effect |-> u.effect, digest |-> u.digest,
                          approval |-> u.approval]).r = "ok")
ReachMoney == ~(\E t \in st.table : t.effect \in Native /\ t.used)

PassedAfterRelease == \A k \in DOMAIN st.passed : st.passed[k].rel
EveryIdHasRow == \A d \in st.sent : \A e \in d.entries : \E x \in st.rows : x.approval = e.approval
RowsAreSent == \A x \in st.rows : x.answer = "allow_once" =>
                    \/ st.items[x.item].st \in {"answered", "recorded"}
                    \/ \E d \in st.sent : d.item = x.item /\ d.via = "release"
=============================================================================
