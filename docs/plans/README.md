# Implementation plans

**Role:** completable checklists for an active slice.  
Not freezes (those are [`../specs/`](../specs/)). Not the capability matrix.

## Rules

- Prefer date-prefixed filenames: `YYYY-MM-DD-topic.md`.  
- At most **one** open plan for the active slice (point at it from
  [`CURRENT.md`](../../CURRENT.md)).  
- Completed plans stay here as history.  
- Many older plans still live as `docs/specs/*-plan.md` — treat those as
  historical; **new** plans go in this directory. Do not invent a third living
  tracker.

## Active

- [2026-08-13-unified-sidebar-plan.md](2026-08-13-unified-sidebar-plan.md) —
  unify kit sidebars on browser etch look (`SidebarPanel` / items); worktree
  landed on master 2026-08-13
- [2026-08-05-distribution-qemu-image-plan.md](2026-08-05-distribution-qemu-image-plan.md) —
  distribution installer (qcow e2e **done** on master; **ISO e2e** + TZ auto +
  Shape 1 refresh still open)
