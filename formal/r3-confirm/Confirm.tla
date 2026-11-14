------------------------------ MODULE Confirm ------------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS Effects, Approvals, Digests, MaxRebinds, PaymentMayRun, BeginUnguarded, Seed, Count

VARIABLES cfg, eff, val, mem, alive, given, wire, seen

vars == <<cfg, eff, val, mem, alive, given, wire, seen>>

Autonomies == <<"observe", "draft_and_confirm", "unattended">>
Classes == <<"read", "prepare", "local_edit", "send", "delete", "submit", "purchase", "payment", "sign", "credential">>
Allowable == <<"delete", "send", "submit">>
YesNo == <<"yes", "no">>
States == <<"none", "refused", "cleared", "held", "rejected", "approved", "releasing", "committed", "unknown">>

Members(q) == {q[i] : i \in DOMAIN q}

ApprovalClasses == {"purchase", "payment", "sign"}
Barred == {"purchase", "payment", "sign", "credential"}
NoEffect == {"read", "prepare", "credential"}
Finals == {"refused", "rejected", "committed", "unknown"}
Limit == 2
CapsSpace == [untrusted : Members(YesNo), private : Members(YesNo), external : Members(YesNo)]

Decide(aut, cls, allow, caps, ck) ==
  IF cls = "credential" THEN "handback"
  ELSE IF aut = "observe" /\ cls # "read" THEN "refuse"
  ELSE IF cls \in {"read", "prepare"} THEN "run"
  ELSE IF cls = "local_edit" /\ ck = "yes" THEN "run"
  ELSE IF cls = "local_edit" /\ ck = "no" THEN "refuse"
  ELSE IF cls \in allow /\ aut = "unattended" /\ (caps.untrusted = "no" \/ caps.private = "no") THEN "run"
  ELSE "confirm"

Mv(k, dec, id, dig, cls, used) == [k |-> k, dec |-> dec, eff |-> id, dig |-> dig, cls |-> cls, used |-> used]
Bare(k) == Mv(k, "-", "-", "-", "-", "-")

Accept(s) == [answer |-> "ok", st |-> s]
Refuse(x, c) == [answer |-> c, st |-> x.st]

Step(x, m) ==
  CASE m.k = "propose" ->
         IF x.st # "none" THEN Refuse(x, "effect")
         ELSE IF x.cls \in NoEffect THEN Refuse(x, "effect")
         ELSE IF m.dec = "refuse" THEN Accept("refused")
         ELSE IF m.dec = "run" THEN Accept("cleared")
         ELSE IF m.dec = "confirm" THEN Accept("held")
         ELSE Refuse(x, "effect")
    [] m.k = "rebind" ->
         IF x.st \notin {"held", "approved"} THEN Refuse(x, "effect")
         ELSE IF m.cls # x.cls THEN Refuse(x, "class")
         ELSE IF m.dig = x.dig THEN Refuse(x, "effect")
         ELSE IF x.reb >= Limit THEN Refuse(x, "changed")
         ELSE Accept("held")
    [] m.k = "approve" ->
         IF x.st # "held" THEN Refuse(x, "effect")
         ELSE IF m.eff # x.id THEN Refuse(x, "approval")
         ELSE IF m.dig # x.dig THEN Refuse(x, "digest")
         ELSE IF m.used = "yes" THEN Refuse(x, "used")
         ELSE Accept("approved")
    [] m.k = "reject" ->
         IF x.st # "held" THEN Refuse(x, "effect")
         ELSE Accept("rejected")
    [] m.k = "end" ->
         IF x.st \notin {"held", "cleared", "approved"} THEN Refuse(x, "effect")
         ELSE Accept("rejected")
    [] m.k = "begin" ->
         IF x.st \in {"none", "refused", "rejected", "committed", "releasing"} THEN Refuse(x, "effect")
         ELSE IF x.st = "unknown" THEN Refuse(x, "unknown")
         ELSE IF x.st = "held" THEN Refuse(x, "approval")
         ELSE IF x.st = "cleared" /\ x.cls \in ApprovalClasses /\ ~BeginUnguarded THEN Refuse(x, "approval")
         ELSE IF m.dig # x.dig THEN Refuse(x, "digest")
         ELSE Accept("releasing")
    [] m.k = "commit" ->
         IF x.st # "releasing" THEN Refuse(x, "effect")
         ELSE Accept("committed")
    [] m.k = "lose" ->
         IF x.st # "releasing" THEN Refuse(x, "effect")
         ELSE Accept("unknown")

Rebound(m, r) == m.k = "rebind" /\ r.answer = "ok"
DigestAfter(x, m, r) == IF Rebound(m, r) THEN m.dig ELSE x.dig
CountAfter(x, m, r) == IF Rebound(m, r) THEN x.reb + 1 ELSE x.reb

Other(c) == IF c = "send" THEN "delete" ELSE "send"

Moves(c) ==
  <<Mv("propose", "run", "-", "-", "-", "-"), Mv("propose", "confirm", "-", "-", "-", "-"),
    Mv("propose", "handback", "-", "-", "-", "-"), Mv("propose", "refuse", "-", "-", "-", "-"),
    Mv("rebind", "-", "-", "d1", c, "-"), Mv("rebind", "-", "-", "d1", Other(c), "-"),
    Mv("rebind", "-", "-", "d2", c, "-"), Mv("rebind", "-", "-", "d2", Other(c), "-"),
    Mv("approve", "-", "e1", "d1", "-", "no"), Mv("approve", "-", "e1", "d1", "-", "yes"),
    Mv("approve", "-", "e1", "d2", "-", "no"), Mv("approve", "-", "e1", "d2", "-", "yes"),
    Mv("approve", "-", "e2", "d1", "-", "no"), Mv("approve", "-", "e2", "d1", "-", "yes"),
    Mv("approve", "-", "e2", "d2", "-", "no"), Mv("approve", "-", "e2", "d2", "-", "yes"),
    Bare("reject"), Bare("end"),
    Mv("begin", "-", "-", "d1", "-", "-"), Mv("begin", "-", "-", "d2", "-", "-"),
    Bare("commit"), Bare("lose")>>

Kind(c) == IF c \in ApprovalClasses THEN "payment" ELSE IF c \in NoEffect THEN "read" ELSE "send"

ASSUME KindIsEnough ==
  \A c \in Members(Classes), s \in Members(States), d \in {"d1", "d2"}, n \in 0..Limit :
    \A i \in DOMAIN Moves(c) :
      Step([id |-> "e1", st |-> s, cls |-> c, dig |-> d, reb |-> n], Moves(c)[i])
        = Step([id |-> "e1", st |-> s, cls |-> Kind(c), dig |-> d, reb |-> n], Moves(Kind(c))[i])

Norm(c) ==
  LET live == c.aut = "unattended" /\ (c.caps.untrusted = "no" \/ c.caps.private = "no") /\ c.allow # {}
  IN [st |-> c.st,
      aut |-> c.aut,
      allow |-> IF live THEN c.allow ELSE {},
      caps |-> IF live THEN [untrusted |-> "no", private |-> "no", external |-> "no"]
                       ELSE [untrusted |-> "yes", private |-> "yes", external |-> "no"]]

ASSUME NormKeepsDecisions ==
  \A aut \in Members(Autonomies), allow \in SUBSET Members(Allowable), caps \in CapsSpace :
    LET c == [st |-> "ok", aut |-> aut, allow |-> allow, caps |-> caps]
        n == Norm(c)
    IN /\ n.aut = c.aut
       /\ n.allow \subseteq c.allow
       /\ Norm(n) = n
       /\ \A cls \in Members(Classes), ck \in Members(YesNo) :
            Decide(c.aut, cls, c.allow, c.caps, ck) = Decide(n.aut, cls, n.allow, n.caps, ck)

Init ==
  /\ cfg \in [st : {"raw"}, aut : Members(Autonomies), allow : SUBSET Members(Classes), caps : CapsSpace]
  /\ eff = [e \in Effects |-> [st |-> "none", cls |-> "none", dig |-> "none", apr |-> "none", reb |-> 0, was |-> {}]]
  /\ val = [e \in Effects |-> "none"]
  /\ mem = [e \in Effects |-> "idle"]
  /\ alive = "yes"
  /\ given = [a \in Approvals |-> [eff |-> "none", dig |-> "none", ans |-> "none"]]
  /\ wire = [e \in Effects |-> <<>>]
  /\ seen = [e \in Effects |-> [cls |-> "none", decs |-> {}, approved |-> <<>>, rel |-> <<>>, left |-> {}, com |-> {}]]

Parse ==
  /\ cfg.st = "raw"
  /\ cfg' = [cfg EXCEPT !.st = IF cfg.allow \subseteq Members(Allowable) THEN "ok" ELSE "malformed"]
  /\ UNCHANGED <<eff, val, mem, alive, given, wire, seen>>

Table(c, ck) == Decide(cfg.aut, c, cfg.allow, cfg.caps, ck)

Proposals(c, ck) ==
  IF PaymentMayRun /\ c = "payment" THEN {Table(c, ck), "run"} ELSE {Table(c, ck)}

Alike(b, c) ==
  /\ Kind(b) = Kind(c)
  /\ \A ck \in Members(YesNo) : Proposals(b, ck) = Proposals(c, ck)

Lead(c) ==
  Classes[CHOOSE i \in DOMAIN Classes : Alike(Classes[i], c) /\ \A j \in 1..(i - 1) : ~Alike(Classes[j], c)]

Row(e) == [id |-> e, st |-> eff[e].st, cls |-> eff[e].cls, dig |-> eff[e].dig, reb |-> eff[e].reb]

LastShown(h) == IF Len(h.approved) = 0 THEN "none" ELSE h.approved[Len(h.approved)].shown

Noted(h, x, y, a, w) ==
  [h EXCEPT
     !.left = IF x.st \in Finals /\ y.st # x.st THEN @ \cup {x.st} ELSE @,
     !.approved = IF y.st = "approved" /\ y # x /\ Len(@) < 3
                  THEN Append(@, [apr |-> a,
                                  ans |-> IF a \in Approvals THEN given[a].ans ELSE "none",
                                  eff |-> IF a \in Approvals THEN given[a].eff ELSE "none",
                                  shown |-> IF a \in Approvals THEN given[a].dig ELSE "none",
                                  had |-> y.dig])
                  ELSE @,
     !.rel = IF y.st = "releasing" /\ x.st # "releasing" /\ Len(@) < 2
             THEN Append(@, [from |-> x.st, dig |-> y.dig])
             ELSE @,
     !.com = IF y.st = "committed" /\ x.st # "committed"
             THEN @ \cup {IF \E i \in DOMAIN w : w[i].reply = "yes" THEN "replied"
                          ELSE IF Len(w) > 0 THEN "sent" ELSE "unsent"}
             ELSE @]

Propose(e, c, ck, d) ==
  /\ alive = "yes"
  /\ cfg.st = "ok"
  /\ cfg = Norm(cfg)
  /\ \E dec \in Proposals(c, ck) :
       LET r == Step([id |-> e, st |-> eff[e].st, cls |-> c, dig |-> d, reb |-> 0], Mv("propose", dec, "-", "-", "-", "-"))
       IN /\ r.answer = "ok"
          /\ eff' = [eff EXCEPT ![e] = [st |-> r.st, cls |-> c, dig |-> d, apr |-> "none", reb |-> 0, was |-> {}]]
  /\ c = Lead(c)
  /\ val' = [val EXCEPT ![e] = d]
  /\ seen' = [seen EXCEPT ![e] = Noted([@ EXCEPT !.cls = c, !.decs = @ \cup {Table(c, ck)}], eff[e], eff'[e], "none", wire[e])]
  /\ UNCHANGED <<cfg, mem, alive, given, wire>>

Change(e, d) ==
  /\ alive = "yes"
  /\ eff[e].st \in {"held", "approved", "cleared"}
  /\ d # val[e]
  /\ val' = [val EXCEPT ![e] = d]
  /\ UNCHANGED <<cfg, eff, mem, alive, given, wire, seen>>

Rebind(e) ==
  /\ alive = "yes"
  /\ eff[e].reb < MaxRebinds
  /\ LET r == Step(Row(e), Mv("rebind", "-", "-", val[e], eff[e].cls, "-"))
     IN /\ r.answer = "ok"
        /\ eff' = [eff EXCEPT ![e].st = r.st, ![e].dig = val[e], ![e].apr = "none", ![e].reb = @ + 1]
  /\ seen' = [seen EXCEPT ![e] = Noted(@, eff[e], eff'[e], "none", wire[e])]
  /\ UNCHANGED <<cfg, val, mem, alive, given, wire>>

Give(a, e, ans) ==
  /\ alive = "yes"
  /\ given[a].ans = "none"
  /\ eff[e].st = "held"
  /\ given' = [given EXCEPT ![a] = [eff |-> e, dig |-> eff[e].dig, ans |-> ans]]
  /\ UNCHANGED <<cfg, eff, val, mem, alive, wire, seen>>

Used(a) == IF \E x \in Effects : a \in eff[x].was THEN "yes" ELSE "no"

Approve(e, a) ==
  /\ alive = "yes"
  /\ given[a].ans = "allow"
  /\ LET r == Step(Row(e), Mv("approve", "-", given[a].eff, given[a].dig, "-", Used(a)))
     IN /\ r.answer = "ok"
        /\ eff' = [eff EXCEPT ![e].st = r.st, ![e].apr = a, ![e].was = @ \cup {a}]
  /\ seen' = [seen EXCEPT ![e] = Noted(@, eff[e], eff'[e], a, wire[e])]
  /\ UNCHANGED <<cfg, val, mem, alive, given, wire>>

Plain(e, m) ==
  /\ LET r == Step(Row(e), m)
     IN /\ r.answer = "ok"
        /\ eff' = [eff EXCEPT ![e].st = r.st]
  /\ seen' = [seen EXCEPT ![e] = Noted(@, eff[e], eff'[e], "none", wire[e])]

Reject(e) ==
  /\ alive = "yes"
  /\ Plain(e, Bare("reject"))
  /\ UNCHANGED <<cfg, val, mem, alive, given, wire>>

End(e) == Plain(e, Bare("end")) /\ UNCHANGED <<cfg, val, mem, alive, given, wire>>

Lose(e) ==
  /\ alive = "no"
  /\ Plain(e, Bare("lose"))
  /\ UNCHANGED <<cfg, val, mem, alive, given, wire>>

Begin(e) ==
  /\ alive = "yes"
  /\ Plain(e, Mv("begin", "-", "-", val[e], "-", "-"))
  /\ mem' = [mem EXCEPT ![e] = "begun"]
  /\ UNCHANGED <<cfg, val, alive, given, wire>>

Send(e) ==
  /\ alive = "yes"
  /\ mem[e] = "begun"
  /\ Len(wire[e]) < 2
  /\ wire' = [wire EXCEPT ![e] = Append(@, [dig |-> val[e], row |-> eff[e].st, after |-> seen[e].rel,
                                           shown |-> LastShown(seen[e]), reply |-> "no", alive |-> alive])]
  /\ mem' = [mem EXCEPT ![e] = "sent"]
  /\ UNCHANGED <<cfg, eff, val, alive, given, seen>>

Reply(e) ==
  /\ alive = "yes"
  /\ mem[e] = "sent"
  /\ Len(wire[e]) > 0
  /\ wire' = [wire EXCEPT ![e][Len(wire[e])].reply = "yes"]
  /\ mem' = [mem EXCEPT ![e] = "replied"]
  /\ UNCHANGED <<cfg, eff, val, alive, given, seen>>

Commit(e) ==
  /\ alive = "yes"
  /\ mem[e] = "replied"
  /\ Plain(e, Bare("commit"))
  /\ mem' = [mem EXCEPT ![e] = "idle"]
  /\ UNCHANGED <<cfg, val, alive, given, wire>>

RunEnds ==
  /\ alive = "yes"
  /\ cfg.st = "ok"
  /\ cfg = Norm(cfg)
  /\ alive' = "no"
  /\ mem' = [e \in Effects |-> "idle"]
  /\ UNCHANGED <<cfg, eff, val, given, wire, seen>>

Next ==
  \/ Parse
  \/ RunEnds
  \/ \E e \in Effects :
       \/ \E c \in Members(Classes), ck \in Members(YesNo), d \in Digests : Propose(e, c, ck, d)
       \/ \E d \in Digests : Change(e, d)
       \/ Rebind(e)
       \/ \E a \in Approvals, ans \in {"allow", "reject"} : Give(a, e, ans)
       \/ \E a \in Approvals : Approve(e, a)
       \/ Reject(e)
       \/ End(e)
       \/ Begin(e)
       \/ Send(e)
       \/ Reply(e)
       \/ Commit(e)
       \/ Lose(e)

Symmetric == Permutations(Effects) \cup Permutations(Approvals) \cup Permutations(Digests)

Arrivals == UNION {{<<e, i>> : i \in DOMAIN seen[e].approved} : e \in Effects}

P1 ==
  \A e \in Effects :
    seen[e].cls \in ApprovalClasses =>
      /\ \A i \in DOMAIN seen[e].rel : seen[e].rel[i].from = "approved"
      /\ \A i \in DOMAIN wire[e] : wire[e][i].after # <<>>

P2 ==
  \A p \in Arrivals :
    LET x == seen[p[1]].approved[p[2]]
    IN x.ans = "allow" /\ x.eff = p[1] /\ x.shown = x.had

P3 ==
  \A a \in Approvals :
    Cardinality({p \in Arrivals : seen[p[1]].approved[p[2]].apr = a}) <= 1

P4 ==
  \A e \in Effects :
    /\ seen[e].left = {}
    /\ Len(seen[e].rel) <= 1
    /\ Len(wire[e]) <= 1

P5 ==
  cfg.aut = "observe" =>
    \A e \in Effects : seen[e].rel = <<>> /\ wire[e] = <<>>

P6 ==
  \A e \in Effects :
    seen[e].decs \cap {"handback", "refuse"} # {} => seen[e].rel = <<>> /\ wire[e] = <<>>

P7 ==
  (cfg.st = "ok" \/ \E e \in Effects : seen[e].cls # "none") => cfg.allow \cap Barred = {}

P8 ==
  \A e \in Effects :
    "confirm" \in seen[e].decs =>
      /\ \A i \in DOMAIN seen[e].rel : seen[e].rel[i].from = "approved"
      /\ \A i \in DOMAIN wire[e] : wire[e][i].after # <<>>

P9 ==
  \A e \in Effects : seen[e].com \subseteq {"replied"}

P10 ==
  \A e \in Effects :
    \A i \in DOMAIN wire[e] :
      LET w == wire[e][i]
      IN /\ w.row = "releasing"
         /\ w.alive = "yes"
         /\ Len(w.after) > 0
         /\ w.dig = w.after[Len(w.after)].dig
         /\ "confirm" \in seen[e].decs => w.dig = w.shown

P11 ==
  \A e \in Effects :
    Len(seen[e].rel) > 0 => eff[e].st \in {"releasing", "committed", "unknown"}

Yn(v) == IF v = "yes" THEN "y" ELSE "n"

RECURSIVE Joined(_)
Joined(q) == IF Len(q) = 1 THEN q[1] ELSE q[1] \o "," \o Joined(Tail(q))

AllowText(allow) ==
  LET q == SelectSeq(Allowable, LAMBDA c : c \in allow)
  IN IF Len(q) = 0 THEN "-" ELSE Joined(q)

TableRow(aut, cls, allow, caps, ck) ==
  aut \o " " \o cls \o " " \o AllowText(allow) \o " "
      \o Yn(caps.untrusted) \o Yn(caps.private) \o Yn(caps.external)
      \o " " \o ck \o " " \o Decide(aut, cls, allow, caps, ck)

PrintTable(u) ==
  \A aut \in Members(Autonomies), cls \in Members(Classes), allow \in SUBSET Members(Allowable),
     caps \in CapsSpace, ck \in Members(YesNo) :
       PrintT("T " \o TableRow(aut, cls, allow, caps, ck))

Confirmed == Mv("propose", "confirm", "-", "-", "-", "-")
Allowed == Mv("approve", "-", "e1", "d1", "-", "no")
Begun == Mv("begin", "-", "-", "d1", "-", "-")
Away(c) == Mv("rebind", "-", "-", "d2", c, "-")
Back(c) == Mv("rebind", "-", "-", "d1", c, "-")

Places == States \o <<"held at the limit", "approved at the limit">>

Path(c, s) ==
  CASE s = "none" -> <<>>
    [] s = "refused" -> <<Mv("propose", "refuse", "-", "-", "-", "-")>>
    [] s = "cleared" -> <<Mv("propose", "run", "-", "-", "-", "-")>>
    [] s = "held" -> <<Confirmed>>
    [] s = "rejected" -> <<Confirmed, Bare("reject")>>
    [] s = "approved" -> <<Confirmed, Allowed>>
    [] s = "releasing" -> <<Confirmed, Allowed, Begun>>
    [] s = "committed" -> <<Confirmed, Allowed, Begun, Bare("commit")>>
    [] s = "unknown" -> <<Confirmed, Allowed, Begun, Bare("lose")>>
    [] s = "held at the limit" -> <<Confirmed, Away(c), Back(c)>>
    [] s = "approved at the limit" -> <<Confirmed, Allowed, Away(c), Back(c), Allowed>>

MaxMoves == 12

Fresh(c, d) == [id |-> "e1", st |-> "none", cls |-> c, dig |-> d, reb |-> 0]

After(x, m, r) == [id |-> x.id, st |-> r.st, cls |-> x.cls, dig |-> DigestAfter(x, m, r), reb |-> CountAfter(x, m, r)]

RECURSIVE Stood(_, _, _)
Stood(x, moves, i) ==
  IF i > Len(moves) THEN x ELSE Stood(After(x, moves[i], Step(x, moves[i])), moves, i + 1)

RECURSIVE LastMove(_, _, _, _)
LastMove(c, x, q, room) ==
  IF q = Len(Moves(c)) \/ room = 1 \/ Step(x, Moves(c)[q]).answer = "ok"
  THEN q
  ELSE LastMove(c, x, q + 1, room - 1)

RECURSIVE Sweep(_, _, _)
Sweep(c, s, p) ==
  IF p > Len(Moves(c)) THEN <<>>
  ELSE LET q == LastMove(c, Stood(Fresh(c, "d1"), Path(c, s), 1), p, MaxMoves - Len(Path(c, s)))
       IN <<[cls |-> c, dig |-> "d1", moves |-> Path(c, s) \o SubSeq(Moves(c), p, q)]>> \o Sweep(c, s, q + 1)

RECURSIVE Flat(_)
Flat(qq) == IF Len(qq) = 0 THEN <<>> ELSE Head(qq) \o Flat(Tail(qq))

Reached(c) == IF c \in NoEffect THEN <<"none">> ELSE Places

Directed(u) ==
  Flat([i \in 1..Len(Classes) |->
          Flat([j \in 1..Len(Reached(Classes[i])) |-> Sweep(Classes[i], Reached(Classes[i])[j], 1)])])

Modulus == 46337
Squared(x) == (x * x + 12345) % Modulus
Hash(i, j) == Squared((Squared((Squared((Seed + 7 * i) % Modulus) + 131 * j) % Modulus) + i + j) % Modulus)

RECURSIVE Drawn(_, _, _, _)
Drawn(x, t, j, n) ==
  IF j > n THEN <<>>
  ELSE LET h == Hash(t, 2 + j)
           all == Moves(x.cls)
           open == SelectSeq(all, LAMBDA m : Step(x, m).answer = "ok")
           pick == IF h % 100 < 60 /\ Len(open) > 0
                   THEN open[1 + ((h \div 100) % Len(open))]
                   ELSE all[1 + ((h \div 100) % Len(all))]
       IN <<pick>> \o Drawn(After(x, pick, Step(x, pick)), t, j + 1, n)

Random(t) ==
  LET c == Classes[1 + (Hash(t, 0) % Len(Classes))]
      d == IF Hash(t, 1) % 2 = 0 THEN "d1" ELSE "d2"
      n == 4 + (Hash(t, 2) % (MaxMoves - 3))
  IN [cls |-> c, dig |-> d, moves |-> Drawn(Fresh(c, d), t, 1, n)]

Quoted(s) == "\"" \o s \o "\""
Optional(s) == IF s = "-" THEN "null" ELSE Quoted(s)
StateText(s) == IF s = "none" THEN "null" ELSE Quoted(s)

MoveText(x, m, r) ==
  "{" \o Quoted("class") \o ":" \o Quoted(x.cls)
      \o "," \o Quoted("move") \o ":" \o Quoted(m.k)
      \o "," \o Quoted("decision") \o ":" \o Optional(m.dec)
      \o "," \o Quoted("entry") \o ":" \o Optional(m.eff)
      \o "," \o Quoted("digest") \o ":" \o Optional(m.dig)
      \o "," \o Quoted("bound_class") \o ":" \o Optional(m.cls)
      \o "," \o Quoted("used") \o ":" \o Optional(m.used)
      \o "," \o Quoted("answer") \o ":" \o Quoted(r.answer)
      \o "," \o Quoted("state") \o ":" \o StateText(r.st)
      \o "," \o Quoted("bound") \o ":" \o Quoted(DigestAfter(x, m, r)) \o "}"

RECURSIVE Played(_, _, _)
Played(x, moves, i) ==
  IF i > Len(moves) THEN <<>>
  ELSE LET m == moves[i]
           r == Step(x, m)
       IN <<MoveText(x, m, r)>> \o Played(After(x, m, r), moves, i + 1)

TraceText(t, n) ==
  "{" \o Quoted("n") \o ":" \o ToString(n)
      \o "," \o Quoted("effect") \o ":" \o Quoted("e1")
      \o "," \o Quoted("bound") \o ":" \o Quoted(t.dig)
      \o "," \o Quoted("moves") \o ":[" \o Joined(Played(Fresh(t.cls, t.dig), t.moves, 1)) \o "]}"

Padded(n) ==
  IF n < 10 THEN "000" \o ToString(n)
  ELSE IF n < 100 THEN "00" \o ToString(n)
  ELSE IF n < 1000 THEN "0" \o ToString(n)
  ELSE ToString(n)

PrintTraces(u) ==
  LET directed == Directed(u)
  IN /\ Assert(Count >= Len(directed), "count is below the number of directed traces")
     /\ PrintT("D " \o ToString(Len(directed)))
     /\ PrintT("J " \o Padded(0) \o " {" \o Quoted("seed") \o ":" \o ToString(Seed) \o "," \o Quoted("count") \o ":" \o ToString(Count) \o "}")
     /\ \A n \in 1..Count :
          PrintT("J " \o Padded(n) \o " " \o TraceText(IF n <= Len(directed) THEN directed[n] ELSE Random(n - Len(directed)), n))

ExportInit ==
  /\ cfg = "export"
  /\ eff = "export"
  /\ val = "export"
  /\ mem = "export"
  /\ alive = "export"
  /\ given = "export"
  /\ wire = "export"
  /\ seen = "export"
  /\ PrintTable(0)
  /\ PrintTraces(0)

ExportNext == UNCHANGED vars

=============================================================================
