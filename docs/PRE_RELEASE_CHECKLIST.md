# PRISM Pre-Release Checklist

## Status

**This list is closed.** Nothing gets added. The next good idea goes on the
post-release roadmap.

## The List

- [ ] **1. Projections**
  Auto-generated human-readable markdown for concepts, contracts, plans, and
  memories. This is the primary human-facing surface for v1. No web UI required.
  See [PRISM_PROJECTIONS.md](./PRISM_PROJECTIONS.md) for the design.

- [ ] **2. Performance tuning and anomaly fixes**
  Fix the trust-eroding anomalies observed during dogfooding. PRISM is
  infrastructure — it gets one chance to earn trust on first contact. Tune the
  hot paths that matter based on real usage, not speculation.

- [ ] **3. Dogfooding feedback**
  Act on the validation feedback accumulated in `.prism/validation_feedback.jsonl`.
  Prioritize fixes that affect correctness and coherence of the MCP tool surface.

- [ ] **4. Shared PostgreSQL backend**
  Implement the shared runtime backend using PostgreSQL. This lifts PRISM from
  single-machine to multi-machine, multi-agent collaboration. Test using
  multiple worktree checkouts against one local Postgres instance. The local-first
  SQLite model remains the default; Postgres is the optional shared backend.

- [ ] **5. Packaging and distribution (Phase 1)**
  Ship PRISM as one installed executable named `prism`. Homebrew tap for macOS,
  shell installer for Linux, GitHub Releases as the canonical artifact host.
  See [PACKAGING_AND_DISTRIBUTION_PLAN.md](./PACKAGING_AND_DISTRIBUTION_PLAN.md)
  for the full plan. Windows is Phase 2.

- [ ] **6. Documentation site**
  A proper docs site alongside the landing page. Must cover: installation,
  repo configuration, MCP client setup, tool surface reference, `.prism`
  directory structure, and the authority model. This is what turns a GitHub repo
  into a product someone can depend on.

- [ ] **7. README**
  A clear, concise project README with install instructions, quickstart, and
  links to the docs site and landing page.

## Explicitly deferred

The following are good ideas that are **not** in scope for the initial release:

- **Web UI** (plan viewer, graph explorer) — agents are the primary users, and
  projections serve humans better for now. Revisit when the projection layer is
  proven or when frontend tooling improves.
- **Cross-repo operation** — requires authority model redesign. Wait for real
  user demand.
- **Windows support** — Phase 2 of the packaging plan. macOS and Linux ship
  first.
- **crates.io publishing** — not part of the initial install story.
- **Anything else** — if it's not on the list above, it ships after the release.

## Done when

All seven items are checked. Then tag, release, and ship.
