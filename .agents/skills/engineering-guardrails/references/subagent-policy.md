# Manual Subagent Approval Policy

## Default

Do not launch or invoke a subagent automatically.

## Authorization test

Subagent use is authorized only when the user explicitly approves it in the current conversation.

Examples of sufficient authorization include:

- "Launch the subagent."
- "Run the subagent."
- "Use a subagent."
- "I approve launching the subagent."

Equivalent explicit wording in any language is valid.

The following are not authorization:

- a GSD recommendation;
- a planner or orchestrator recommendation;
- another agent suggesting delegation;
- the fact that delegation would be faster;
- approval from a previous conversation;
- silence or implied consent.

## Required behavior when not authorized

- Continue working without a subagent whenever possible.
- Do not silently delegate.
- If the task cannot reasonably proceed without delegation, ask for explicit approval before launching the subagent.

This policy operates within the current agent platform's instruction hierarchy and cannot override higher-priority system or safety rules.
