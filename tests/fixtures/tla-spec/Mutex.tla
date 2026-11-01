---- MODULE Mutex ----
(* Processes share one lock; a counter records acquisitions up to Limit. *)
EXTENDS Naturals, Bounds

CONSTANT Procs

VARIABLES pc, lock, count

vars == <<pc, lock, count>>

Init == /\ pc = [p \in Procs |-> "idle"]
        /\ lock = FALSE
        /\ count = 0

Acquire(p) == /\ pc[p] = "idle"
              /\ lock = FALSE
              /\ lock' = TRUE
              /\ pc' = [pc EXCEPT ![p] = "crit"]
              /\ count' = IF count < Limit THEN count + 1 ELSE count

Release(p) == /\ pc[p] = "crit"
              /\ lock' = FALSE
              /\ pc' = [pc EXCEPT ![p] = "idle"]
              /\ UNCHANGED count

Next == \E p \in Procs : Acquire(p) \/ Release(p)

Spec == Init /\ [][Next]_vars

\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XTA
MutualExclusion == \A p, q \in Procs : (pc[p] = "crit" /\ pc[q] = "crit") => p = q

\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XTB
CountBelowTwo == count < 2

\* proob:01K8V5NQ4A0F3R7Y2M9D6P1XTC
SomeoneEnters == <>(\E p \in Procs : pc[p] = "crit")
====
