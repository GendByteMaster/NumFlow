# GSD Workflow Integration

Use this reference only when GSD is actually present or explicitly requested.

## Before file-changing work

Prefer the repository's GSD entry points when they exist:

- small fixes, documentation updates, and ad-hoc work: use the local equivalent of `gsd-quick`;
- investigation and bug fixing: use the local equivalent of `gsd-debug`;
- already planned phase work: use the local equivalent of `gsd-execute-phase`.

Command syntax differs between clients and repositories. Detect the available command or skill names instead of assuming `/`, `$`, or any specific invocation syntax.

## Rules

- Keep planning artifacts and execution context synchronized when the repository relies on GSD.
- Do not bypass an established GSD workflow for repository edits unless the user explicitly requests bypass or the repository instructions allow it.
- Do not install or initialize GSD in a repository merely because this skill knows about GSD.
- GSD recommendations do not authorize subagent use. The subagent approval gate in the main skill still applies.
