------------------------------- MODULE Reach -------------------------------
EXTENDS Admit

Lingering ==
    \E ref \in Refs(edits) :
        /\ ref[1] < newest
        /\ Now(At(edits, ref)).state \in {"provisional", "active", "reverted"}

HoldsAny(s) == Loaded(s) /\ \E n \in Steps : Holds(runs[s], n)

Asked(s, n, proposal) == Answer(s, n, proposal, KeyOf(runs[s].base, n), FALSE)

Causes(cause) ==
    \E s \in Slots, n \in Steps \cup {NoStep} :
        /\ Loaded(s)
        /\ \E proposal \in Proposals(n), key \in {KeyOf(runs[s].base, n), OtherKey} :
               Answer(s, n, proposal, key, FALSE).cause = cause

TwoRunsHoldCandidates == \A s \in Slots : HoldsAny(s)

CompileUnderCandidate == newest = 2 /\ \E s \in Slots : HoldsAny(s) /\ runs[s].base = 1

RestartDropsCandidate == ~restarted /\ restarted' /\ \E s \in Slots : HoldsAny(s)

StopLeavesEdits == Lingering

FinishWithdrawsLeftEdits == Lingering /\ ~Lingering'

LoadBeforeWithdrawal == Lingering /\ \E s \in Slots : Loaded(s) /\ runs[s].base = newest

ProposalBeforeWithdrawal == Lingering /\ \E s \in Slots : HoldsAny(s) /\ runs[s].base = newest

LostFinishBeforeWithdrawal ==
    /\ Lingering
    /\ Lingering'
    /\ \E s \in Slots : runs[s].phase = "lost" /\ runs'[s].phase = "idle"

ProposalHoldsCompiled ==
    \E s \in Slots, n \in Steps :
        /\ Loaded(s)
        /\ \E proposal \in Proposals(n) :
               /\ Range(proposal) \cap Compiled # {}
               /\ Asked(s, n, proposal).cause = "ok"
               /\ Range(Asked(s, n, proposal).new) \cap Compiled = {}

ProposalHoldsBarred ==
    \E s \in Slots, n \in Steps :
        /\ Loaded(s)
        /\ \E proposal \in Proposals(n) :
               /\ Range(proposal) \cap Barred(edits, newest, n) # {}
               /\ Asked(s, n, proposal).cause = "rejected"

ProposalHoldsListed ==
    \E s \in Slots, n \in Steps :
        /\ Loaded(s)
        /\ Base(edits, newest, n) # <<>>
        /\ \E proposal \in Proposals(n) :
               LET listed == Range(At(edits, Base(edits, newest, n)).list)
               IN /\ Range(proposal) \cap listed # {}
                  /\ Asked(s, n, proposal).cause = "ok"
                  /\ Range(Asked(s, n, proposal).new) \cap listed = {}

CandidateListedByBase ==
    \E s \in Slots, n \in Steps :
        /\ Loaded(s)
        /\ Holds(runs[s], n)
        /\ runs[s].base = newest
        /\ Base(edits, newest, n) # <<>>
        /\ Left(edits, runs[s], n) = <<>>

ListCut ==
    \E ref \in Refs(edits) :
        LET edit == At(edits, ref)
        IN /\ edit.on # <<>>
           /\ Len(edit.list) = ListMax
           /\ ~(Range(At(edits, edit.on).list) \subseteq Range(edit.list))

EditLimit == \E b \in Bases, n \in Steps : Len(edits[b][n]) = EditMax

ActiveInForceAgain ==
    \E ref \in Refs(edits) :
        LET edit == At(edits, ref)
        IN /\ Now(edit).state = "reverted"
           /\ edit.on # <<>>
           /\ InForce(edits, newest, ref[2]) = edit.on

EditsOnBothSteps == \A n \in Steps : \E b \in Bases : edits[b][n] # <<>>

UnverifiedRunReverts ==
    \E ref \in Refs(edits) :
        /\ Now(At(edits, ref)).state = "provisional"
        /\ Now(At(edits', ref)).state = "reverted"
        /\ ~Now(At(edits', ref)).verified

CountedOnActive ==
    \E s \in Slots, n \in Steps :
        /\ Ends(s)
        /\ n \in last'.counted
        /\ runs[s].served[n] # <<>>
        /\ runs[s].served[n][1] = newest
        /\ Now(At(edits, runs[s].served[n])).state = "active"
        /\ At(edits', runs[s].served[n]) = At(edits, runs[s].served[n])

StaleProposalRefused ==
    \E s \in Slots, n \in Steps :
        /\ Loaded(s)
        /\ runs[s].base < newest
        /\ \E proposal \in Proposals(n) : Asked(s, n, proposal).cause = "base"

LowerServedNoMove ==
    \E s \in Slots, n \in Steps :
        /\ Ends(s)
        /\ n \in last'.counted
        /\ runs[s].served[n] # <<>>
        /\ runs[s].served[n][1] < newest
        /\ Len(Added(runs[s].served[n])) = 1
        /\ Now(At(edits', runs[s].served[n])).state = "withdrawn"

CauseBase == Causes("base")

CauseScope == Causes("scope")

CauseLocator == Causes("locator")

CauseRejected == Causes("rejected")

CauseFull == Causes("full")

NotTwoRunsHoldCandidates == ~TwoRunsHoldCandidates

NotCompileUnderCandidate == ~CompileUnderCandidate

NeverRestartDropsCandidate == [][~RestartDropsCandidate]_vars

NotStopLeavesEdits == ~StopLeavesEdits

NeverFinishWithdrawsLeftEdits == [][~FinishWithdrawsLeftEdits]_vars

NotLoadBeforeWithdrawal == ~LoadBeforeWithdrawal

NotProposalBeforeWithdrawal == ~ProposalBeforeWithdrawal

NeverLostFinishBeforeWithdrawal == [][~LostFinishBeforeWithdrawal]_vars

NotProposalHoldsCompiled == ~ProposalHoldsCompiled

NotProposalHoldsBarred == ~ProposalHoldsBarred

NotProposalHoldsListed == ~ProposalHoldsListed

NotCandidateListedByBase == ~CandidateListedByBase

NotListCut == ~ListCut

NotEditLimit == ~EditLimit

NotActiveInForceAgain == ~ActiveInForceAgain

NotEditsOnBothSteps == ~EditsOnBothSteps

NeverUnverifiedRunReverts == [][~UnverifiedRunReverts]_vars

NeverCountedOnActive == [][~CountedOnActive]_vars

NotStaleProposalRefused == ~StaleProposalRefused

NeverLowerServedNoMove == [][~LowerServedNoMove]_vars

NotCauseBase == ~CauseBase

NotCauseScope == ~CauseScope

NotCauseLocator == ~CauseLocator

NotCauseRejected == ~CauseRejected

NotCauseFull == ~CauseFull

=============================================================================
