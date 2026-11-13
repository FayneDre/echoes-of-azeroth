---
name: new-adr
description: Record an architecture decision for Echoes of Azeroth as an ADR in docs/decisions/. Use when a structural choice is made or changed (technology, data model, distribution, security, scope), or when the user says "on part sur X" for something architectural.
---

# Record an architecture decision

1. Read `docs/decisions/README.md` and the existing ADRs; pick the next number.
   If the decision replaces an older one, the old ADR's status becomes
   `Superseded by NNNN` (the only allowed edit to an accepted ADR).
2. Create `docs/decisions/NNNN-kebab-title.md` (English):
   ```markdown
   # NNNN. Title
   - Status: Accepted
   - Date: YYYY-MM-DD
   ## Context
   Facts and constraints, with sources for upstream facts.
   ## Decision
   What we do, precisely enough to implement.
   ## Consequences
   Trade-offs, follow-up work, what becomes harder.
   ```
3. Add the row to the table in `docs/decisions/README.md`.
4. Propagate: update `CLAUDE.md` §5 (key decisions) and, if the model or
   pipeline changed, `docs/architecture.md` and `docs/roadmap.md`.
5. No personal information (names, e-mails) anywhere: the repository is public.
