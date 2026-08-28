# Commit Diff Analysis and Conventional Commit Guidance

Use this policy whenever the user asks to analyze a git diff, prepare a commit, generate a commit message, or summarize changes for a commit.

The actual diff is the primary source of truth. Repository/task context may explain intent only when it is explicitly available and consistent with the diff.

## 1. Analyze the diff carefully

Determine the primary type of change from the actual diff:

- `feat` — new feature or capability;
- `fix` — bug fix;
- `refactor` — code change without intended behavior change;
- `perf` — performance improvement;
- `docs` — documentation-only change;
- `style` — formatting or style-only change;
- `test` — tests only or primarily test changes;
- `chore` — maintenance that does not fit the categories above.

Use another Conventional Commit type only when the repository already uses it and the diff clearly supports it.

If the diff contains multiple materially unrelated changes, state that clearly and recommend splitting the work into separate commits when appropriate.

## 2. Generate the Conventional Commit subject

Preferred format:

```text
<type>(<scope>): <short summary>
```

When no meaningful scope can be inferred, use:

```text
<type>: <short summary>
```

Rules:

- maximum 72 characters unless the repository defines a stricter limit;
- use imperative mood, for example `add`, `fix`, `update`, `remove`, or `refactor`;
- be specific and concise;
- choose scope from the affected module/service/package/domain only when supported by the diff;
- do not invent functionality, motivation, impact, or scope not present in the evidence.

## 3. Write the mandatory structured Description

The `Description` section is **mandatory and must never be omitted**, even when the user asks only for a commit message.

Always output this structure:

```text
Description:
- Changes:
  - ...

- Reason:
  - ...

- Implementation Details:
  - ...

- Impact / Risks:
  - ...

- Breaking Changes:
  - None
```

### Changes

Clearly list what was modified based on the diff, including relevant:

- files;
- modules/services/packages;
- logic or behavior;
- contracts or interfaces;
- configuration;
- tests;
- repository structure.

Do not list changes that are not visible in the diff or otherwise supplied as reliable evidence.

### Reason

Explain why the change was made **only when the reason is inferable from the diff or explicitly supplied task context**.

If the reason is not inferable, write that plainly, for example:

```text
- Reason is not inferable from the diff alone.
```

Never manufacture intent to make the description sound more complete.

### Implementation Details

Describe key technical decisions or logic visible in the diff, such as:

- important control-flow changes;
- data-flow changes;
- API/contract changes;
- dependency or configuration changes;
- concurrency, persistence, validation, error-handling, or lifecycle changes;
- relevant architecture decisions directly evidenced by the diff.

Do not describe implementation details that cannot be supported by the actual changes.

### Impact / Risks

Identify direct or reasonably implied side effects and behavior changes supported by the diff.

Examples can include:

- compatibility effects;
- changed runtime behavior;
- altered data flow;
- new failure modes;
- migration requirements;
- changed operational behavior.

Do not invent speculative production incidents or unsupported guarantees.

If no meaningful risk can be inferred, say so explicitly.

### Breaking Changes

Describe actual breaking changes when present.

Otherwise always write:

```text
- None
```

Do not classify a change as breaking merely because implementation details changed internally.

## 4. Evidence and accuracy rules

- Do **not** invent functionality not present in the diff.
- Do **not** invent the reason for a change.
- Do **not** claim tests passed unless test evidence is available.
- Do **not** claim a build, migration, deployment, benchmark, or verification succeeded without evidence.
- Do **not** infer a security improvement, compatibility guarantee, performance improvement, or production effect unless the diff or supplied evidence supports it.
- Distinguish facts visible in the diff from contextual inference.
- When evidence is incomplete, state the limitation explicitly instead of filling the gap.

## 5. Output language

Output the commit subject and complete mandatory `Description` in clear, professional English unless the user explicitly requests another language.

## Required output shape

```text
<type>(<scope>): <short summary>

Description:
- Changes:
  - ...

- Reason:
  - ...

- Implementation Details:
  - ...

- Impact / Risks:
  - ...

- Breaking Changes:
  - None
```

The `Description` section is mandatory and must not be omitted.
