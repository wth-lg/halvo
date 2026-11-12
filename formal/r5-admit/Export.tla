------------------------------- MODULE Export -------------------------------
EXTENDS Admit

CONSTANTS Seed, Count

ASSUME Seed \in 0..999999999 /\ Count \in 200..1000

Openings ==
    <<<<>>,
      <<"propose_refused">>,
      <<"propose_ok">>,
      <<"propose_ok", "drop">>,
      <<"propose_ok", "keep">>,
      <<"propose_ok", "keep", "use_ok">>,
      <<"propose_ok", "keep", "use_ok", "use_ok">>,
      <<"propose_ok", "keep", "use_fail_counted">>,
      <<"propose_ok", "keep", "use_ok", "use_fail_counted">>,
      <<"propose_ok", "keep", "recompile">>,
      <<"propose_ok", "keep", "use_ok", "use_ok", "recompile">>,
      <<"propose_ok", "keep", "use_fail_counted", "recompile">>>>

Paths ==
    [none |-> <<>>,
     candidate |-> <<"propose_ok">>,
     provisional |-> <<"propose_ok", "keep">>,
     active |-> <<"propose_ok", "keep", "use_ok", "use_ok">>,
     reverted |-> <<"propose_ok", "keep", "use_fail_counted">>,
     withdrawn |-> <<"propose_ok", "keep", "recompile">>,
     refused |-> <<"propose_refused">>]

RECURSIVE Uses(_)
Uses(clean) == IF clean = 0 THEN <<>> ELSE <<"use_ok">> \o Uses(clean - 1)

Pairs == Len(States) * Len(Moves)

Targets == Pairs + (K - 1)

Target(t) ==
    IF t <= Pairs
    THEN <<States[1 + ((t - 1) \div Len(Moves))], Moves[1 + ((t - 1) % Len(Moves))], 0>>
    ELSE <<"provisional", "use_ok", t - Pairs>>

Lcg(x) ==
    LET high == 48271 * (x % 44488)
        low == 3399 * (x \div 44488)
    IN IF high > low THEN high - low ELSE high + (2147483647 - low)

Draw(x, n) == (x \div 65536) % n

RECURSIVE Walk(_, _, _, _, _, _)
Walk(state, clean, x, opening, left, told) ==
    IF left = 0 THEN told
    ELSE LET coin == Lcg(x)
             next == Lcg(coin)
             taken == SelectSeq(Moves, LAMBDA m : EditStep(state, m, clean).answer # "edit")
             move == IF opening # <<>> THEN Head(opening)
                     ELSE IF Draw(coin, 2) = 0 /\ taken # <<>>
                          THEN taken[1 + Draw(next, Len(taken))]
                     ELSE Moves[1 + Draw(next, Len(Moves))]
             step == EditStep(state, move, clean)
             after == IF step.answer = "edit" THEN "null" ELSE ToString(step.clean)
         IN Walk(step.state, step.clean, next,
                 IF opening # <<>> THEN Tail(opening) ELSE opening, left - 1,
                 Append(told, <<move, Guard(state, move, clean), step.answer, after>>))

Trace(t) ==
    LET first == Lcg(Lcg(1 + ((Seed + 1000003 * t) % 2147483646)))
        second == Lcg(first)
        target == Target(t)
        opening == IF t <= Targets
                   THEN (Paths[target[1]] \o Uses(target[3])) \o <<target[2]>>
                   ELSE Openings[1 + Draw(first, Len(Openings))]
    IN Walk("none", 0, second, opening, 6 + Draw(second, 11), <<>>)

RECURSIVE Spell(_)
Spell(told) ==
    IF told = <<>> THEN ""
    ELSE " " \o told[1][1] \o ":" \o told[1][2] \o ":" \o told[1][3] \o ":" \o told[1][4]
             \o Spell(Tail(told))

Cleans(state, move) == {clean \in 0..K : clean = 0 \/ Guard(state, move, clean) # "-"}

After(state, move, clean) ==
    LET step == EditStep(state, move, clean)
    IN IF step.answer = "edit" THEN "-"
       ELSE IF \A passed \in 0..K : EditStep(state, move, passed).clean = passed THEN "same"
       ELSE ToString(step.clean)

RowLines ==
    UNION {UNION {{"row " \o States[i] \o " " \o Moves[j] \o " "
                       \o Guard(States[i], Moves[j], clean) \o " "
                       \o EditStep(States[i], Moves[j], clean).answer \o " "
                       \o After(States[i], Moves[j], clean) :
                           clean \in Cleans(States[i], Moves[j])} :
                      j \in DOMAIN Moves} :
               i \in DOMAIN States}

ExportInit ==
    /\ Init
    /\ \A line \in RowLines : PrintT(line)
    /\ \A t \in 1..Count : PrintT("trace " \o ToString(t) \o Spell(Trace(t)))

ExportNext == UNCHANGED vars

=============================================================================
