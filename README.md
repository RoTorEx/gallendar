# gallendar

<!-- VIBE:PROJECT_BRIEF_REQUIRED -->

TODO: explain what this project does, for whom, and how to use it.

## Project truth

- `BUSINESS.md` — purpose, concepts, invariants, decisions, and business-module map.
- `business/*` — focused business domains, created only when needed.
- `CHANGELOG.md` — notable unreleased and released behavior.
- `AGENTS.md` — context router for coding agents.

## Kernel

Refresh the committed operating rules explicitly:

```bash
make vibe-pull
```

If the parent path moved:

```bash
make vibe-kernel-set KERNEL=/absolute/path/to/vibecoding-kernel
make vibe-pull
```
