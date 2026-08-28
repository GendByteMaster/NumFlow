# Goal Intelligence Policy

Use this policy when the user explicitly requests goal-backed work or when substantial work is too ambiguous to have an honest, measurable completion condition.

Do not force goal creation for ordinary implementation tasks that already have clear acceptance criteria.

## 1. Need check

Before defining a goal, decide whether a goal adds useful control.

Use Goal Intelligence when one or more of these are true:

- the user asks to define, create, refine, or work from a goal;
- the requested outcome is broad enough that success is not yet measurable;
- multiple plausible interpretations would lead to materially different implementations;
- verification, scope boundaries, or stop conditions are not yet clear for substantial work.

Skip persistent goal creation when the task is already narrow, measurable, and directly executable.

## 2. Goal shape

A strong goal should define:

- **Outcome** — the concrete state that should be true when the work is complete;
- **Target** — the relevant artifact, repository area, system, environment, or user-visible behavior;
- **Evidence** — the commands, tests, measurements, reviews, or observations that can prove completion;
- **Success threshold** — a binary or quantitative bar for success;
- **In scope** — the work allowed or required to reach the outcome;
- **Out of scope** — boundaries that prevent accidental expansion when ambiguity matters;
- **Stop / escalate condition** — the condition that should cause the agent to stop and ask instead of continuing blindly.

Prefer goals that describe an observable result rather than activity.

## 3. Goal quality bar

Before treating a goal as ready, verify that it answers these questions:

1. What concrete result should exist when the work is done?
2. What evidence can prove that result?
3. What binary or quantitative threshold defines success?
4. What scope boundaries materially constrain the work?
5. What condition should make the agent stop, escalate, or ask the user?

If one of these is missing but can be safely inferred from repository context, sharpen the goal without adding unnecessary ceremony.

If the missing information can change the intended outcome or validator, ask one concise clarification question.

## 4. Quantification

Use numbers only when they represent real engineering success. Do not add decorative precision.

Useful quantitative evidence can include:

- exact test, lint, typecheck, build, CI, or acceptance commands and their required pass condition;
- latency, throughput, memory, bundle size, error rate, cost, accuracy, coverage, flake rate, uptime, or other relevant thresholds;
- a required number of successful repetitions or verification runs;
- a bounded set of files, modules, routes, services, records, environments, or review comments;
- explicit time windows or monitoring windows for operational changes;
- maximum blast radius or allowed failure count when the domain supports it.

When meaningful quantification is unavailable, use the strongest honest binary validator instead.

## 5. Domain heuristics

### Bugs

Prefer a goal that includes:

- reproduction or a known failing condition;
- the smallest safe fix scope;
- a regression validator that fails before the fix when practical and passes after it.

### Tests and verification

Name the exact relevant command or acceptance condition when known. Success should mean the required validator passes, not merely that tests were edited.

### Performance

Define the metric, target threshold, measurement method, and repetition count when repeated measurements are needed for confidence.

### Quality and reliability

Use observable acceptance evidence such as reviewed examples, static checks, test coverage, error-rate limits, or repository-specific quality gates.

### Research and architecture

Define the decision the research must enable, the systems or sources in scope, and the evidence standard needed to make that decision.

### Operations

Define the expected healthy state, verification or monitoring window, failure threshold, rollback trigger, and escalation condition when relevant.

## 6. Weak-goal detection

Do not accept activity-only goals as complete goal definitions.

Examples of weak goal shapes include:

- "make progress";
- "improve the project";
- "keep investigating";
- "work on performance";
- "clean things up".

Rewrite them into an observable result when repository and task context make the intended result clear.

If multiple materially different outcomes remain plausible, ask one focused clarification question instead of guessing.

## 7. Clarification discipline

Ask only when the missing information can change the desired outcome, validation method, environment, or important scope boundary.

Prefer a single concise question over a questionnaire.

When the user cannot provide a metric, propose the strongest honest binary validator available and make the uncertainty explicit.

## 8. Existing goal state

When the current environment provides persistent goal-management capabilities:

1. inspect existing active goal state before creating another goal;
2. reuse an unfinished goal when it still matches the user's intent;
3. surface a conflict when an active goal materially differs from the new request;
4. let the user decide whether to complete, replace, abandon, or separate conflicting goal-backed work when that decision matters;
5. create new persistent goal state only when no suitable active goal already exists.

Never assume tool names or persistent goal APIs exist.

## 9. Portability

ForgeGuard must remain portable across coding agents and repositories.

- If goal-management tools exist, use them according to their actual contract.
- If they do not exist, keep the goal in the current task context or use the repository's established planning mechanism.
- Do not fabricate `get_goal`, `create_goal`, snapshot, ledger, decision-log, or resume capabilities.
- Do not introduce persistent planning artifacts merely because Goal Intelligence supports structured goals.
- Do not set a token budget unless the user explicitly requests one.

## 10. Recommended local goal format

When no persistent goal tool is available, use a compact structure like:

```text
Goal:
- Outcome: ...
- Success Criteria: ...
- Evidence: ...
- In Scope: ...
- Out of Scope: ...
- Stop / Escalate: ...
```

Keep it proportional to the task. Goal Intelligence exists to make substantial work measurable, not to add bureaucracy to routine changes.
