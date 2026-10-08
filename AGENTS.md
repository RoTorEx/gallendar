# AGENTS.md

Keep this file a short context router. Put product truth in `BUSINESS.md` and
focused `business/*` modules instead of expanding this file.

## Kernel routing

<!-- VIBE:KERNEL_ROUTING_START -->

This project uses committed local copies of the Vibecoding Kernel.

- Always read `.vibe/kernel/OPERATING.md` and choose its maintenance, operation,
  or audit mode before normal project work.
- For version, tag, publish, or release work, also read
  `.vibe/kernel/RELEASE.md`.
- For user corrections, instruction conflicts, reusable agent-workflow lessons,
  kernel proposals, or a newly pulled kernel version, also read
  `.vibe/kernel/EVOLUTION.md`.
- For product behavior or domain logic, read `BUSINESS.md` and only the relevant
  module from `business/*`.
- Do not edit `.vibe/kernel/*` manually or read the parent kernel during normal
  work. Refresh the local copy with `make vibe-pull`.
- When a reusable workflow improvement belongs in the parent, run
  `make vibe-propose`; it appends a reviewable proposal without changing rules.

<!-- VIBE:KERNEL_ROUTING_END -->

## Local rules

- Preserve durable user explanations and corrections in the appropriate local
  source of truth during the same task.
- Use `.vibe/kernel/EVOLUTION.md` for reusable workflow feedback; never block a
  local fix while waiting for a parent-kernel decision.
- Add project-specific routing here only when it prevents repeated mistakes or
  unnecessary context loading.

## First project brief

<!-- VIBE:PROJECT_BRIEF_BOOTSTRAP_START -->

When `README.md` or `BUSINESS.md` contains `VIBE:PROJECT_BRIEF_REQUIRED`, resolve
the brief before the first substantive implementation:

- use the current user request when it already explains the user, problem, and
  useful outcome;
- otherwise ask one short question covering those three points;
- write only known facts into README and BUSINESS; do not invent business rules;
- remove both brief markers and this entire bootstrap section when complete.

<!-- VIBE:PROJECT_BRIEF_BOOTSTRAP_END -->
