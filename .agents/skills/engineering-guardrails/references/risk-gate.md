# Risk Gate

Use this policy before executing changes or operations whose failure could affect data, security, compatibility, availability, money, or production systems.

The purpose of the Risk Gate is not to block ordinary engineering work. It is to make blast radius, reversibility, verification, and approval requirements explicit before consequential actions.

## 1. Classify risk

Classify the operation using the highest applicable level.

### Low

Typical examples:

- documentation-only changes;
- test-only changes that do not alter production behavior;
- local refactors with no public contract change;
- reversible development-only configuration changes;
- narrowly scoped UI or internal implementation changes with no sensitive boundary impact.

Expected behavior:

- proceed normally;
- run focused verification;
- report meaningful residual risk if any.

### Medium

Typical examples:

- behavior changes inside an existing public feature;
- dependency upgrades with runtime impact;
- non-destructive schema additions;
- internal API or event-contract changes with known consumers;
- feature-flag, caching, queue, retry, or concurrency changes;
- infrastructure configuration that can affect a non-production environment.

Expected behavior:

- identify affected components and rollback path;
- verify compatibility and relevant tests;
- prefer incremental or feature-flagged rollout where available;
- surface uncertainty instead of silently assuming safety.

### High

Typical examples:

- authentication, authorization, permissions, session, identity, or secret-handling changes;
- cryptographic or key-management changes;
- payment, billing, quota, entitlement, or money-related logic;
- production deployment configuration;
- database migrations that transform or rewrite existing data;
- externally visible breaking API/event/schema changes;
- durability, distributed-consistency, idempotency, or recovery semantics that can lose or duplicate state;
- changes that widen access to protected data or services.

Expected behavior:

- explicitly state why the operation is High risk;
- define blast radius, failure modes, rollback or recovery plan, and verification evidence;
- prepare code/configuration when appropriate, but do not execute an irreversible or production-impacting step without explicit user authorization in the current conversation;
- prefer staged rollout, compatibility windows, backups, canaries, dry-runs, or reversible toggles where available;
- if a safe rollback cannot be described, escalate the operation to Critical.

### Critical

Typical examples:

- destructive production database operations such as dropping/truncating data or irreversible bulk mutation;
- irreversible migrations without a tested recovery path;
- production data deletion or mass account/permission changes;
- credential, signing-key, encryption-key, or trust-root rotation that could cause widespread lockout or data loss;
- disabling security controls in production;
- deployment with very large blast radius and no safe rollback;
- breaking distributed protocol/data changes without backward compatibility where partial rollout can corrupt or strand state.

Expected behavior:

- do not execute automatically;
- require explicit user approval immediately before execution;
- require a concrete backup/recovery or rollback strategy where technically possible;
- verify the exact target/environment and scope before execution;
- prefer a dry-run, staged operation, backup, snapshot, maintenance window, or canary before the irreversible step;
- if required recovery evidence is unavailable, stop and report that the operation is not safe to execute.

## 2. Preparation is different from execution

A High or Critical classification does not automatically forbid writing code, migration files, deployment manifests, or runbooks.

The approval gate applies to the consequential execution step, for example:

- applying a destructive migration;
- deploying to production;
- rotating production keys;
- deleting production data;
- activating a breaking contract;
- changing live permissions or security policy.

The agent may prepare and review the change beforehand unless another instruction forbids it.

## 3. Escalation rules

Escalate to the next level when any of these are true:

- the target environment is uncertain;
- the blast radius is larger than initially understood;
- rollback is unavailable or untested;
- backups or recovery evidence are missing;
- the change crosses authentication, authorization, cryptography, money, or production-data boundaries;
- partial rollout could create incompatible state;
- verification cannot establish whether the operation succeeded safely.

Never lower a risk level merely to avoid an approval gate.

## 4. Completion report

For Medium, High, and Critical work, the completion report should state when relevant:

- assigned risk level;
- affected systems/data;
- verification performed;
- rollback/recovery status;
- whether a production/destructive execution step was performed or only prepared;
- unresolved risks or required follow-up.
