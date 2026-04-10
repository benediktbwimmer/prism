# Native `prism_code` Builder And Compiler Phase 7b

Status: in progress  
Audience: prism-js, prism-mcp, prism-core, coordination, compiler, and runtime maintainers  
Scope: replace the transitional `prism.mutate(...)` bridge with native source-level `prism_code`
builders, handles, and staged lowering for coordination authoring

---

## 1. Goal

Phase 7b exists because the public `prism_code` cutover is not the same thing as the native
authoring model.

Today, write-capable `prism_code` is real, but the public author still has to think in terms of:

- `prism.mutate(...)`
- tagged actions
- mutation payload shapes

That transport bridge was acceptable for the public cutover, but it is not the intended
programming model.

Phase 7b closes that gap.

The target is:

- native builder objects and handles inside `prism_code`
- lexical bindings instead of authored client ids
- one-call staged lowering into native coordination transactions
- source-level authoring for plan and task creation, updates, graph extension, and direct claim /
  artifact lifecycle writes

## 2. Required outcomes

Phase 7b is complete only when all of the following are true:

- `prism_code` can author coordination graph changes without public `prism.mutate(...)`
- one invocation still equals one transaction boundary in v1
- builders and handles stage native changes during execution and commit once at the end
- public write errors are source-level and object-level, not action-schema-level
- public docs and examples teach native `prism_code` builders first
- `prism.mutate(...)` is no longer required for normal coordination authoring flows

This phase does not require the full final compiler for every later roadmap feature.

It does require:

- one real builder/lowering core that later phases can extend
- the first native authoring slices for the current coordination graph model

## 3. First implementation slices

### Slice 1: Native staged coordination transaction context

Deliver:

- one per-invocation write context for `prism_code`
- staged coordination transaction assembly during code execution
- internal ephemeral ids or handles only inside lowering
- automatic commit or dry-run at the end of the invocation

Success condition:

- `prism_code` can create and stage multiple related coordination mutations in one invocation
  without exposing public mutation payload stitching

Status note (2026-04-10):

- landed: one per-invocation staged coordination transaction context now commits or dry-runs at the
  end of the `prism_code` invocation
- landed: internal handle identities stay inside the lowering layer
- landed: native `prism.work.declare(...)` so declared-work bootstrap no longer needs a public
  `prism.mutate(...)` call
- remaining: broaden the native builder surface beyond the first coordination slices

### Slice 2: Native plan and task authoring handles

Deliver:

- plan creation through a native builder API
- task creation through native plan-scoped APIs
- dependency wiring through object handles
- source-level results that return created plans and tasks directly

Success condition:

- users and agents can create a plan, create tasks, and express dependencies without
  `prism.mutate(...)`

Status note (2026-04-10):

- landed: `prism.coordination.createPlan(...)`
- landed: `prism.coordination.openPlan(...)`
- landed: `plan.update(...)`
- landed: `plan.archive()`
- landed: `plan.addTask(...)`
  - now supports richer native task authoring fields including `assignee`, `anchors`,
    `acceptance`, `artifactRequirements`, and `reviewRequirements`
- landed: `task.dependsOn(...)`

### Slice 3: Native extension and update slices

Deliver:

- live-plan extension APIs
- task update or completion-style APIs
- native object-scoped methods that compose with the same staged transaction context

Success condition:

- the normal coordination lifecycle no longer requires the old mutation action catalog on the
  public `prism_code` surface

Status note (2026-04-10):

- landed: live-plan extension through `prism.coordination.openPlan(...)`
- landed: task reopen/update/complete through `prism.coordination.openTask(...)`,
  `task.update(...)`, and `task.complete(...)`
- landed: richer native task update fields through `task.update(...)`, including assignee,
  priority, validation refs, dependency rewiring, and artifact/review requirements
- landed: first direct native task lifecycle helpers through `task.handoff(...)`,
  `task.acceptHandoff(...)`, `task.resume(...)`, and `task.reclaim(...)`
- landed: native claim lifecycle helpers through `prism.claim.acquire(...)`,
  `prism.claim.renew(...)`, and `prism.claim.release(...)`
- landed: native artifact lifecycle helpers through `prism.artifact.propose(...)`,
  `prism.artifact.review(...)`, and `prism.artifact.supersede(...)`
- remaining: broaden native lifecycle coverage so normal coordination work no longer needs the
  legacy `prism.mutate(...)` escape hatch at all

## 4. Hard rules

The implementation must preserve these constraints:

- one `prism_code` invocation is still one transaction boundary in v1
- no public authored client ids
- no public `prism.mutate(...)` requirement for normal coordination authoring
- lowering may reuse existing native coordination transaction machinery internally
- the public API must stay source-level and handle-oriented
- dry-run must exercise the same lowering path and skip only the final commit

## 5. Validation

Minimum validation during the phase:

- `cargo test -p prism-js`
- targeted `cargo test -p prism-mcp server_tool_calls`
- targeted `cargo test -p prism-mcp coordination_surface`

Also run `cargo test -p prism-cli` when public `prism_code` MCP behavior or schema-facing docs
change in ways the CLI consumes.

## 6. Exit note

Phase 7b should be judged by whether a new caller can treat `prism_code` as a real native
authoring surface.

If the recommended answer to “how do I create or extend a plan?” is still “call
`prism.mutate(...)` with the right action payload,” this phase is not done.
