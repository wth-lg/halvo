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
Table == {"run", "confirm"}
NoApproval == -1
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
    [n |-> 0,
     shown |-> [i \in Items |-> {}],
     moves |-> [i \in Items |-> <<>>],
     answers |-> {},
     minted |-> [i \in Items |-> {}],
     stored |-> {},
     acked |-> {},
     got |-> {},
     released |-> {},
     passed |-> {},
     refused |-> {},
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

No(s, r) == [r |-> r, st |-> s, out |-> <<>>]
Ids(ds) == {d.approval : d \in ds}

Issue(s, ok) ==
    LET n == Len(s.items) + 1
        it == [req |-> n, st |-> "shown", ans |-> "none", ids |-> {}]
    IN IF s.run # "live" THEN No(s, "state")
       ELSE IF OpenItems(s) # {} THEN No([s EXCEPT !.run = "ended"], "state")
       ELSE IF ok
       THEN [r |-> "ok", st |-> [s EXCEPT !.items = Append(@, it)],
             out |-> <<[k |-> "item_raised", item |-> n]>>]
       ELSE [r |-> "ok",
             st |-> [s EXCEPT !.items = Append(@, [it EXCEPT !.st = "lapsed"]),
                              !.nap = @ + Cardinality(ReqEffects(n))],
             out |-> <<[k |-> "answer_confirm", item |-> n, answer |-> "reject", ids |-> Ids(Mint(s, n))],
                       [k |-> "sent", item |-> n, via |-> "lapse",
                        entries |-> Entries(n, Mint(s, n), "reject_once")],
                       [k |-> "item_lapsed", item |-> n, cause |-> "digest"]>>]

Arm(s, i) ==
    IF i \notin DOMAIN s.items THEN No(s, "state")
    ELSE IF s.items[i].st # "shown" THEN No(s, "state")
    ELSE [r |-> "ok", st |-> [s EXCEPT !.items[i].st = "armed"], out |-> <<>>]

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
                  out |-> <<[k |-> "answer_confirm", item |-> i, answer |-> ans, ids |-> Ids(ids)]>>]

Record(s, i) ==
    IF i \notin DOMAIN s.items THEN No(s, "state")
    ELSE IF s.items[i].st # "answered" THEN No(s, "state")
    ELSE LET rows == Rows(i, ItemEntries(s.items[i]))
         IN [r |-> "ok",
             st |-> [s EXCEPT !.items[i].st = "recorded",
                              !.table = @ \cup {[row |-> x, live |-> FALSE, used |-> FALSE] : x \in rows}],
             out |-> <<[k |-> "item_answered", item |-> i, rows |-> rows]>>]

Release(s, i) ==
    IF i \notin DOMAIN s.items THEN No(s, "state")
    ELSE IF s.items[i].st # "recorded" THEN No(s, "state")
    ELSE LET es == ItemEntries(s.items[i])
         IN [r |-> "ok",
             st |-> [s EXCEPT !.items[i].st = "released",
                              !.table = {IF t.row.item = i THEN [t EXCEPT !.live = TRUE] ELSE t : t \in @}],
             out |-> <<[k |-> "sent", item |-> i, via |-> "release", entries |-> es],
                       [k |-> "item_released", item |-> i, entries |-> es]>>]

LapseOk(it, cause) ==
    \/ it.st \in {"shown", "armed"}
    \/ it.st = "answered" /\ cause \in {"run_ended", "cancelled", "no_record", "app_ended"}
    \/ it.st = "recorded" /\ cause \in {"run_ended", "app_ended"}

Lapse(s, i, cause) ==
    IF i \notin DOMAIN s.items THEN No(s, "state")
    ELSE IF ~LapseOk(s.items[i], cause) THEN No(s, "state")
    ELSE LET it == s.items[i]
             gone == cause \in {"run_ended", "app_ended"}
             stop == cause \in {"wait_limit", "cancelled"}
             es == Entries(it.req, Mint(s, it.req), "reject_once")
         IN [r |-> "ok",
             st |-> [s EXCEPT !.items[i].st = "lapsed",
                              !.nap = IF gone THEN @ ELSE @ + Cardinality(ReqEffects(it.req)),
                              !.table = {t \in @ : t.row.item # i},
                              !.run = IF gone THEN "ended" ELSE IF stop THEN "stopped" ELSE @,
                              !.app = IF cause = "app_ended" THEN "ended" ELSE @],
             out |-> (IF stop THEN <<[k |-> "cancel"]>> ELSE <<>>)
                     \o (IF gone \/ it.st \notin {"shown", "armed"} THEN <<>>
                      ELSE <<[k |-> "answer_confirm", item |-> i, answer |-> "reject", ids |-> Ids(Mint(s, it.req))]>>)
                     \o (IF it.st = "answered"
                         THEN <<[k |-> "unacked_rows", item |-> i, rows |-> Rows(i, ItemEntries(it))]>> ELSE <<>>)
                     \o (IF gone THEN <<>> ELSE <<[k |-> "sent", item |-> i, via |-> "lapse", entries |-> es]>>)
                     \o <<[k |-> "item_lapsed", item |-> i, cause |-> cause]>>]

Stop(s) ==
    IF s.run # "live" \/ OpenItems(s) # {} THEN No(s, "state")
    ELSE [r |-> "ok", st |-> [s EXCEPT !.run = "stopped"], out |-> <<[k |-> "cancel"]>>]

Guard(s, rec) ==
    LET pass == [r |-> "ok", st |-> s, out |-> <<[k |-> "pass"]>>]
        m == {t \in s.table : /\ t.row.approval = rec.approval
                               /\ t.row.effect = rec.effect
                               /\ t.row.digest = rec.digest
                               /\ t.row.answer = "allow_once"
                               /\ t.live}
    IN IF rec.state \notin {"approved", "releasing"} THEN No(s, "ok")
       ELSE IF s.run = "stopped" THEN No(s, "approval")
       ELSE IF rec.state = "approved"
       THEN IF m = {} THEN No(s, "approval")
            ELSE IF {t \in m : ~t.used} = {} THEN [pass EXCEPT !.r = "repeat"]
            ELSE [pass EXCEPT !.st = [s EXCEPT !.table = (@ \ m) \cup {[t EXCEPT !.used = TRUE] : t \in m}]]
       ELSE IF rec.effect \in Native
       THEN IF {t \in m : t.used} = {} THEN No(s, "approval") ELSE pass
       ELSE IF rec.approval = NoApproval
       THEN IF rec.policy = "run" THEN pass ELSE No(s, "approval")
       ELSE IF m = {} THEN No(s, "approval") ELSE pass

Step(s, mv) ==
    CASE mv.m = "issue" -> Issue(s, mv.ok)
      [] mv.m = "arm" -> Arm(s, mv.item)
      [] mv.m = "answer" -> Answer(s, mv.item, mv.answer, mv.from)
      [] mv.m = "record" -> Record(s, mv.item)
      [] mv.m = "release" -> Release(s, mv.item)
      [] mv.m = "lapse" -> Lapse(s, mv.item, mv.cause)
      [] mv.m = "stop" -> Stop(s)
      [] mv.m = "guard" -> Guard(s, mv)

ItemMoves(s, i) ==
    {[m |-> "arm", item |-> i], [m |-> "record", item |-> i], [m |-> "release", item |-> i]}
    \cup {[m |-> "answer", item |-> i, answer |-> x, from |-> y] : x \in Answers, y \in Sources}
    \cup {[m |-> "lapse", item |-> i, cause |-> c, stored |-> FALSE] : c \in Causes}
    \cup {[m |-> "lapse", item |-> i, cause |-> "no_record", stored |-> TRUE]}

GuardMoves(s) ==
    {[m |-> "guard", effect |-> e, state |-> x, digest |-> d, approval |-> p, policy |-> "none"] :
        e \in Effects, x \in RecStates \ {"releasing"}, d \in Digests, p \in 0..s.nap}
    \cup {[m |-> "guard", effect |-> e, state |-> "releasing", digest |-> d, approval |-> p, policy |-> y] :
        e \in Effects, d \in Digests, p \in 0..s.nap, y \in Table}
    \cup {[m |-> "guard", effect |-> e, state |-> "approved", digest |-> Digest(1, e), approval |-> NoApproval,
           policy |-> "none"] : e \in Effects}
    \cup {[m |-> "guard", effect |-> e, state |-> "releasing", digest |-> Digest(1, e), approval |-> NoApproval,
           policy |-> y] : e \in Effects, y \in Table}

Moves(s) ==
    IF s.app = "ended" THEN {}
    ELSE IF s.run = "ended"
    THEN IF OpenItems(s) # {}
         THEN {[m |-> "lapse", item |-> i, cause |-> "run_ended", stored |-> FALSE] : i \in OpenItems(s)}
         ELSE UNION {ItemMoves(s, i) : i \in DOMAIN s.items}
    ELSE (IF Len(s.items) < NRequests THEN {[m |-> "issue", ok |-> b] : b \in BOOLEAN} ELSE {})
         \cup UNION {ItemMoves(s, i) : i \in DOMAIN s.items}
         \cup {[m |-> "stop"]}
         \cup GuardMoves(s)

HLast(g, i) == IF Len(g.moves[i]) = 0 THEN "none" ELSE g.moves[i][Len(g.moves[i])].m
HMade(m, cause) == [m |-> m, cause |-> cause]

HBefore(g, mv) ==
    CASE mv.m = "issue" ->
            LET n == g.n + 1
            IN [g EXCEPT !.n = n,
                         !.shown[n] = {[effect |-> e, digest |-> IF mv.ok THEN Digest(n, e) ELSE 0] :
                                          e \in ReqEffects(n)},
                         !.moves[n] = IF mv.ok THEN <<HMade("issue", "none")>>
                                      ELSE <<HMade("issue", "none"), HMade("lapse", "digest")>>]
      [] mv.m = "answer" ->
            [g EXCEPT !.answers = @ \cup {[item |-> mv.item, answer |-> mv.answer, from |-> mv.from,
                                           armed |-> HLast(g, mv.item) = "arm"]},
                      !.moves[mv.item] = Append(@, HMade("answer", "none"))]
      [] mv.m \in {"arm", "record", "release"} ->
            [g EXCEPT !.moves[mv.item] = Append(@, HMade(mv.m, "none"))]
      [] mv.m = "lapse" -> [g EXCEPT !.moves[mv.item] = Append(@, HMade("lapse", mv.cause))]
      [] OTHER -> g

HEvent(g, e, mv) ==
    CASE e.k = "answer_confirm" ->
            [g EXCEPT !.minted[e.item] = IF mv.m = "answer" THEN @ \cup e.ids ELSE @]
      [] e.k = "item_answered" ->
            IF mv.m = "record" THEN [g EXCEPT !.stored = @ \cup e.rows, !.acked = @ \cup e.rows] ELSE g
      [] e.k = "unacked_rows" ->
            IF mv.cause = "no_record" /\ mv.stored THEN [g EXCEPT !.stored = @ \cup e.rows] ELSE g
      [] e.k = "sent" ->
            [g EXCEPT !.got = @ \cup {[item |-> e.item, via |-> e.via, entries |-> e.entries,
                                       after |-> g.stopped]}]
      [] e.k = "item_released" -> [g EXCEPT !.released = @ \cup {[item |-> e.item, entries |-> e.entries]}]
      [] e.k = "pass" ->
            [g EXCEPT !.passed = @ \cup
                {[state |-> mv.state, effect |-> mv.effect, digest |-> mv.digest, approval |-> mv.approval,
                  policy |-> IF mv.approval = NoApproval THEN mv.policy ELSE "none",
                  first |-> \/ mv.state = "releasing"
                            \/ \A p \in g.passed : ~(p.state = "approved" /\ p.approval = mv.approval),
                  used |-> \E p \in g.passed : /\ p.state = "approved"
                                               /\ p.effect = mv.effect
                                               /\ p.digest = mv.digest
                                               /\ p.approval = mv.approval,
                  stopped |-> g.stopped]}]
      [] OTHER -> g

RECURSIVE HFold(_, _, _, _)
HFold(g, out, i, mv) == IF i > Len(out) THEN g ELSE HFold(HEvent(g, out[i], mv), out, i + 1, mv)

HAfter(g, mv) ==
    CASE mv.m = "lapse" -> [g EXCEPT !.stopped = @ \/ mv.cause \in {"wait_limit", "cancelled"}]
      [] mv.m = "stop" -> [g EXCEPT !.stopped = TRUE]
      [] OTHER -> g

HPassedOn(out) == \E i \in 1..Len(out) : out[i].k = "pass"
HRepeat(g, mv) ==
    /\ mv.state = "approved"
    /\ ~g.stopped
    /\ \E p \in g.passed : /\ p.state = "approved"
                           /\ p.effect = mv.effect
                           /\ p.digest = mv.digest
                           /\ p.approval = mv.approval

HGuard(g, mv, res) ==
    IF HPassedOn(res.out) THEN HFold(g, res.out, 1, mv)
    ELSE IF HRepeat(g, mv)
    THEN [g EXCEPT !.refused = @ \cup {[effect |-> mv.effect, digest |-> mv.digest, approval |-> mv.approval]}]
    ELSE g

Hist(g, mv, res) ==
    IF mv.m = "guard" THEN HGuard(g, mv, res)
    ELSE IF res.r = "ok" /\ mv.m = "lapse" THEN HFold(HAfter(HBefore(g, mv), mv), res.out, 1, mv)
    ELSE IF res.r = "ok" THEN HAfter(HFold(HBefore(g, mv), res.out, 1, mv), mv)
    ELSE HFold(g, res.out, 1, mv)

GotAllow(g, i, e) == \E d \in g.got : /\ d.via = "release"
                                      /\ d.item = i
                                      /\ \E x \in d.entries : x.effect = e /\ x.answer = "allow_once"
Asks(g) == \A i \in 1..g.n : \A e \in ReqEffects(i) \cap ReqEffects(g.n + 1) : GotAllow(g, i, e)
Offered(s, g, mv) == mv.m # "issue" \/ Asks(g) \/ Step(s, mv).r # "ok"

Init == st = Init0 /\ h = InitH
Next == \E mv \in Moves(st) :
            /\ Offered(st, h, mv)
            /\ LET res == Step(st, mv) IN st' = res.st /\ h' = Hist(h, mv, res)

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
        /\ h.moves[i][k].m \in {"release", "lapse"} => k = Len(h.moves[i])
        /\ (h.moves[i][k].m = "lapse" /\ \E j \in 1..(k - 1) : h.moves[i][j].m = "answer")
              => h.moves[i][k].cause \in {"run_ended", "cancelled", "no_record", "app_ended"}
        /\ (h.moves[i][k].m = "lapse" /\ \E j \in 1..(k - 1) : h.moves[i][j].m = "record")
              => h.moves[i][k].cause \in {"run_ended", "app_ended"}
L5 == /\ \A d \in h.got : d.via = "release" =>
            d.entries = {Entry(x) : x \in {y \in h.acked : y.item = d.item}}
      /\ \A x \in h.released :
            \E d \in h.got : d.via = "release" /\ d.item = x.item /\ d.entries = x.entries
L6 == /\ \A d \in h.got : (d.via = "lapse" \/ d.after) => \A e \in d.entries : e.answer = "reject_once"
      /\ \A x \in h.released : ~Lapsed(x.item)
L7 == /\ \A p \in h.passed : p.state = "approved" => Row(p) # {} /\ (p.first \/ p.used)
      /\ h.refused = {}
L8 == \A p \in h.passed : (p.state = "releasing" /\ p.effect \in Native) => Row(p) # {} /\ p.used
L9 == \A p \in h.passed :
        /\ ~p.stopped
        /\ \A d \in h.got : \A e \in d.entries : e.answer = "reject_once" => e.approval # p.approval
L10 == \A p \in h.passed : p.state = "releasing" =>
            IF p.approval = NoApproval THEN p.policy = "run" ELSE Row(p) # {}

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
                          approval |-> t.row.approval, policy |-> "none"]).r = "approval"
            /\ Guard(st, [state |-> "approved", effect |-> u.row.effect, digest |-> u.row.digest,
                          approval |-> u.row.approval, policy |-> "none"]).r = "ok")
ReachMoney == \A p \in h.passed : ~(p.state = "releasing" /\ p.effect \in Native)
ReachPageRecord == \A p \in h.passed : ~(p.state = "releasing" /\ p.effect \notin Native)
ReachOtherEffect ==
    ~(\E t \in st.table : /\ t.live /\ ~t.used /\ t.row.answer = "allow_once"
                          /\ \E e \in Effects \ {t.row.effect} :
                                Guard(st, [state |-> "approved", effect |-> e, digest |-> t.row.digest,
                                           approval |-> t.row.approval, policy |-> "none"]).r = "approval")
ReachOtherApproval ==
    ~(\E t, u \in st.table :
            /\ t.live /\ u.live /\ t.row.effect \notin Native /\ u.row.effect # t.row.effect
            /\ t.row.answer = "allow_once" /\ u.row.answer = "allow_once"
            /\ Guard(st, [state |-> "releasing", effect |-> t.row.effect, digest |-> t.row.digest,
                          approval |-> u.row.approval, policy |-> "confirm"]).r = "approval")
ReachStopLapse == ~(st.run = "stopped" /\ \E i \in Items : Lapsed(i))
ReachStopPerson == ~(st.run = "stopped" /\ \A i \in Items : ~Lapsed(i))
ReachStoppedRequest == ~(st.run = "stopped" /\ Len(st.items) < NRequests /\ Issue(st, TRUE).r = "state")
ReachStoppedRecord ==
    ~(/\ st.run = "stopped"
      /\ \E t \in st.table : /\ t.live /\ ~t.used /\ t.row.answer = "allow_once"
                             /\ Guard(st, [state |-> "approved", effect |-> t.row.effect,
                                           digest |-> t.row.digest, approval |-> t.row.approval, policy |-> "none"]).r = "approval")
ReachRejected == \A d \in h.got : d.via # "release" \/ \A e \in d.entries : e.answer = "allow_once"
ReachRepeat ==
    ~(\E t \in st.table : /\ t.live /\ t.used
                          /\ Guard(st, [state |-> "approved", effect |-> t.row.effect, digest |-> t.row.digest,
                                        approval |-> t.row.approval, policy |-> "none"]).r = "repeat")
ReachUsedRowOther ==
    ~(\E t \in st.table : /\ t.live /\ t.used /\ st.run = "live"
                          /\ \E d \in Digests \ {t.row.digest} :
                                Guard(st, [state |-> "approved", effect |-> t.row.effect, digest |-> d,
                                           approval |-> t.row.approval, policy |-> "none"]).r = "approval")
ReachNoApprovalRun == \A p \in h.passed : ~(p.state = "releasing" /\ p.approval = NoApproval)
ReachNoApprovalRefused ==
    ~(\E e \in Effects \ Native : \E t \in st.table :
            /\ t.live /\ t.row.effect = e /\ t.row.answer = "allow_once" /\ st.run = "live"
            /\ Guard(st, [state |-> "releasing", effect |-> e, digest |-> t.row.digest,
                          approval |-> NoApproval, policy |-> "confirm"]).r = "approval"
            /\ Guard(st, [state |-> "releasing", effect |-> e, digest |-> t.row.digest,
                          approval |-> NoApproval, policy |-> "run"]).r = "ok")
ReachNoApprovalNative ==
    ~(\E e \in Native : \E t \in st.table :
            /\ t.live /\ t.row.effect = e /\ t.row.answer = "allow_once" /\ st.run = "live"
            /\ Guard(st, [state |-> "releasing", effect |-> e, digest |-> t.row.digest,
                          approval |-> NoApproval, policy |-> "run"]).r = "approval")
ReachNotAskedAgain ==
    ~(/\ h.n = 1 /\ OpenItems(st) = {} /\ st.run = "live"
      /\ \E e \in Req1 \cap Req2 : ~GotAllow(h, 1, e)
      /\ Issue(st, TRUE).r = "ok")

AskedOnApproval ==
    \A n \in 2..h.n : \A i \in 1..(n - 1) : \A e \in ReqEffects(i) \cap ReqEffects(n) : GotAllow(h, i, e)

EveryIdHasRow == \A d \in h.got : \A e \in d.entries : \E x \in h.stored : x.approval = e.approval
RowsAreSent == \A x \in h.stored : x.answer = "allow_once" =>
                    \/ \E i \in OpenItems(st) : i = x.item
                    \/ \E d \in h.got : d.item = x.item /\ d.via = "release"
=============================================================================
