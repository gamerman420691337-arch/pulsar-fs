# ADR B3-041: require the creation-aware reader before cgroup intent publication

Status: reader prerequisite gates pass; ADR B3-042 adds the format-4 journal primitive. Admitted creator, production admission and complete recovery remain pending.

A creation roster changes ownership semantics before a cgroup can be registered. Older readers must not ignore that roster and admit new work. Schema 20 therefore requires the exact minimum-ledger-4 binding layout, preserving the same host scope and mandatory active CUDA/worker table requirements. Earlier schema/layout pairs remain exact and independently validated. Removing the new column, lowering the minimum or lowering the schema cannot turn the state back into legacy authority.

Publication shares the authorized caller transaction through a savepoint. Other worker/staging/portable writers must preserve the stronger requirement. Placement initialization and CUDA validators retain and validate the new schema without weakening existing archived-evidence checks. No portable format change follows from a host-local reader requirement.

This increment deliberately has no production admission caller. At this checkpoint the existing ledger could not satisfy minimum 4; an insufficient format-3 ledger still refuses. ADR B3-042 now adds format-4 reader support and native schema-20 reopen, while the admitted creator and production activation remain pending. The format-4 creator must first be implemented with durable pre-mkdir intent, held parent/group identities, failed-acknowledgement custody, conservative ambiguous-resource recovery and registration transfer. Only then may production admission publish the ledger, commit the schema requirement and expose the creator, in that order. The current production mkdir-to-register gap remains open until those obligations are met.

See the [implementation status and compatibility evidence](../../docs/PULSAR_CGROUP_CREATION_READER.md).
