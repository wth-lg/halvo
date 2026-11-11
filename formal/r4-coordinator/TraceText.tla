----------------------------- MODULE TraceText -----------------------------
EXTENDS Integers, Sequences, TLC

Q == "\""
Str(x) == Q \o x \o Q
Id(k, n) == Str(k \o ToString(n))
KV(k, v) == Str(k) \o ":" \o v

RECURSIVE Join(_)
Join(xs) ==
    IF Len(xs) = 0 THEN ""
    ELSE IF Len(xs) = 1 THEN xs[1]
    ELSE xs[1] \o "," \o Join(Tail(xs))

Arr(xs) == "[" \o Join(xs) \o "]"
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
=============================================================================
