-------------------------------- MODULE Hold --------------------------------
EXTENDS Naturals

CONSTANT Null

States == {"new", "held", "sent", "settled", "expired", "voided"}
Final == {"settled", "expired", "voided"}
MoveNames == {"reserve", "dispatch", "void", "settle", "expire"}
Causes == {"not_active", "no_cap", "over_cap", "no_hold", "used", "repeat",
           "other_arguments", "state"}

NewHold == [state |-> "new", amount |-> 0, cost |-> 0]

ToReserve(amount) == [name |-> "reserve", arg |-> amount]
ToDispatch == [name |-> "dispatch", arg |-> Null]
ToVoid == [name |-> "void", arg |-> Null]
ToSettle(cost) == [name |-> "settle", arg |-> cost]
ToExpire == [name |-> "expire", arg |-> Null]

Left(a) == IF a.cap > a.spent + a.held THEN a.cap - (a.spent + a.held) ELSE 0

Refused(cause, a, h) == [answer |-> cause, acct |-> a, hold |-> h]
Passed(a, h) == [answer |-> "ok", acct |-> a, hold |-> h]
Released(a, h) == [a EXCEPT !.held = @ - h.amount]

Step(a, h, m) ==
    CASE m.name = "reserve" ->
            IF h.state # "new"
            THEN Refused(IF m.arg = h.amount THEN "repeat" ELSE "other_arguments", a, h)
            ELSE IF m.arg = 0 THEN Refused("state", a, h)
            ELSE IF ~a.active THEN Refused("not_active", a, h)
            ELSE IF a.cap = Null THEN Refused("no_cap", a, h)
            ELSE IF a.spent + a.held + m.arg > a.cap THEN Refused("over_cap", a, h)
            ELSE Passed([a EXCEPT !.held = @ + m.arg],
                        [h EXCEPT !.state = "held", !.amount = m.arg])
      [] m.name = "dispatch" ->
            IF h.state = "new" THEN Refused("no_hold", a, h)
            ELSE IF h.state # "held" THEN Refused("used", a, h)
            ELSE Passed(a, [h EXCEPT !.state = "sent"])
      [] m.name = "void" ->
            IF h.state = "new" THEN Refused("no_hold", a, h)
            ELSE IF h.state # "held" \/ a.held < h.amount THEN Refused("state", a, h)
            ELSE Passed(Released(a, h), [h EXCEPT !.state = "voided"])
      [] m.name = "settle" ->
            IF h.state = "new" THEN Refused("no_hold", a, h)
            ELSE IF h.state # "sent" \/ a.held < h.amount THEN Refused("state", a, h)
            ELSE Passed([Released(a, h) EXCEPT !.spent = @ + m.arg],
                        [h EXCEPT !.state = "settled", !.cost = m.arg])
      [] m.name = "expire" ->
            IF h.state = "new" THEN Refused("no_hold", a, h)
            ELSE IF h.state \in Final \/ a.held < h.amount THEN Refused("state", a, h)
            ELSE Passed([Released(a, h) EXCEPT !.spent = @ + h.amount],
                        [h EXCEPT !.state = "expired", !.cost = h.amount])

=============================================================================
