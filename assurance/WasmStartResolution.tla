-------------------- MODULE WasmStartResolution --------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANT Actors, References
VARIABLES readAuthority, editAuthority, namespace, preflight,
          resolution, withdrew, unauthorizedResolutions, unauthorizedReplays
vars == <<readAuthority, editAuthority, namespace, preflight,
          resolution, withdrew, unauthorizedResolutions, unauthorizedReplays>>
Empty == [kind |-> "Free", reference |-> "None"]
Absent == [kind |-> "Absent", reference |-> "None"]
Init ==
  /\ readAuthority = Actors
  /\ editAuthority = Actors
  /\ namespace = [a \in Actors |-> Empty]
  /\ preflight = {}
  /\ resolution = [a \in Actors |-> Absent]
  /\ withdrew = {}
  /\ unauthorizedResolutions = 0
  /\ unauthorizedReplays = 0
Authorized(a) == a \in readAuthority /\ a \in editAuthority
Preflight(a, ref) ==
  /\ Authorized(a)
  /\ <<a,ref>> \notin preflight
  /\ preflight' = preflight \cup {<<a,ref>>}
  /\ UNCHANGED <<readAuthority,editAuthority,namespace,resolution,
                 withdrew,unauthorizedResolutions,unauthorizedReplays>>
PublishStart(a, ref) ==
  /\ Authorized(a) /\ <<a,ref>> \in preflight
  /\ namespace[a].kind = "Free"
  /\ namespace' = [namespace EXCEPT ![a] = [kind |-> "Accepted", reference |-> ref]]
  /\ UNCHANGED <<readAuthority,editAuthority,preflight,resolution,
                 withdrew,unauthorizedResolutions,unauthorizedReplays>>
Resolve(a, ref) ==
  /\ Authorized(a)
  /\ resolution[a].kind = "Absent"
  /\ namespace[a].kind = "Free" \/
       (namespace[a].kind = "Accepted" /\ namespace[a].reference = ref)
  /\ namespace' = IF namespace[a].kind = "Free"
       THEN [namespace EXCEPT ![a] = [kind |-> "Withdrawn", reference |-> ref]]
       ELSE namespace
  /\ resolution' = [resolution EXCEPT ![a] =
       [kind |-> IF namespace[a].kind = "Free" THEN "Withdrawn" ELSE "Accepted",
        reference |-> ref]]
  /\ withdrew' = IF namespace[a].kind = "Free" THEN withdrew \cup {a} ELSE withdrew
  /\ unauthorizedResolutions' = unauthorizedResolutions +
       (IF Authorized(a) THEN 0 ELSE 1)
  /\ UNCHANGED <<readAuthority,editAuthority,preflight,unauthorizedReplays>>
Replay(a, ref) ==
  /\ Authorized(a)
  /\ resolution[a].kind # "Absent" /\ resolution[a].reference = ref
  /\ unauthorizedReplays' = unauthorizedReplays + (IF Authorized(a) THEN 0 ELSE 1)
  /\ UNCHANGED <<readAuthority,editAuthority,namespace,preflight,resolution,
                 withdrew,unauthorizedResolutions>>
ChangeRead(a) ==
  /\ readAuthority' = IF a \in readAuthority THEN readAuthority \ {a}
                      ELSE readAuthority \cup {a}
  /\ UNCHANGED <<editAuthority,namespace,preflight,resolution,
                 withdrew,unauthorizedResolutions,unauthorizedReplays>>
ChangeEdit(a) ==
  /\ editAuthority' = IF a \in editAuthority THEN editAuthority \ {a}
                      ELSE editAuthority \cup {a}
  /\ UNCHANGED <<readAuthority,namespace,preflight,resolution,
                 withdrew,unauthorizedResolutions,unauthorizedReplays>>
Next == \E a \in Actors:
          ChangeRead(a) \/ ChangeEdit(a) \/
          (\E ref \in References: Preflight(a,ref) \/ PublishStart(a,ref) \/
                                     Resolve(a,ref) \/ Replay(a,ref))
TypeOK ==
  /\ readAuthority \subseteq Actors /\ editAuthority \subseteq Actors
  /\ namespace \in [Actors -> [kind: {"Free","Accepted","Withdrawn"},
                               reference: References \cup {"None"}]]
  /\ resolution \in [Actors -> [kind: {"Absent","Accepted","Withdrawn"},
                                reference: References \cup {"None"}]]
  /\ preflight \subseteq (Actors \X References)
  /\ withdrew \subseteq Actors
  /\ unauthorizedResolutions \in Nat
  /\ unauthorizedReplays \in Nat
NoStartAfterWithdrawal == \A a \in withdrew: namespace[a].kind = "Withdrawn"
ResolutionCorrespondence == \A a \in Actors:
  resolution[a].kind # "Absent" =>
    (resolution[a].kind = namespace[a].kind /\
     resolution[a].reference = namespace[a].reference)
NoUnauthorizedResolution == unauthorizedResolutions = 0
NoUnauthorizedReplay == unauthorizedReplays = 0
Spec == Init /\ [][Next]_vars
=====================================================================
