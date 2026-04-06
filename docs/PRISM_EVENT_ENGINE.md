# PRISM Event Engine & Recurring Execution Architecture

Status: Draft / V2 Roadmap
Audience: PRISM core maintainers, orchestrator integrators

---

## 1. Summary

While PRISM V1 establishes a mathematically rigorous `Plan/Task` coordination graph, it is fundamentally pull-based (agents or humans poll the graph for actionable work). To support enterprise swarms, continuous integration loops, and push-based agent scaling, PRISM must introduce temporal and event-driven primitives.

This document outlines three architectural pillars for PRISM V2:
1. **Continue As New** (Recurring Plans) to solve infinite graph growth.
2. **The PRISM Hook SDK** to provide a TypeScript-native developer experience for orchestration side-effects.
3. **Event Tombstones** to solve the Thundering Herd deduplication problem in a decentralized, Git-native mesh.

---

## 2. Recurring Executions & The "Continue As New" Pattern

### The Problem
If continuous or scheduled processes (e.g., "Verify Agent-Dev Integrations") merely append new `Task` children to a static `Plan` indefinitely, the graph becomes a memory leak. History bloats local SQLite databases and inflates the Shared Git Ref tree payloads out of proportion.

### The Solution: Plan-level Cycles
Inspired by Temporal's "Continue As New" architecture, recurrence is strictly modeled at the `Plan` boundary, not the `Task` level.

1. A Plan is designated with a `recurringable` lifecycle (e.g., triggered via Cron or Graph Action).
2. The Plan contains its set of constituent tasks.
3. When the final task completes, instead of recycling tasks to `pending` (which destroys the append-only mathematical purity of the graph), the orchestrator triggers a `coordination_transaction`.
4. This transaction:
   - Clones a brand new instance of the Plan (e.g., `Integration V2`) based on the Plan template.
   - Archives the previous Plan (`Integration V1`).
5. **Result:** The DAG graph size remains permanently bounded. `Archived` plans are ejected from hot read-models and local caches, solving the Garbage Collection problem while preserving exact historical audits of prior executions.

---

## 3. The Hook TypeScript SDK

### The Problem
Defining triggers (when to spawn an agent, when to send a Slack alert) via CLI-generated YAML manifests or generic bash scripts creates severe friction. Developers lack autocompletion, type safety, and the ability to codify complex deployment logic.

### The Solution: `.prism/hooks.ts`
PRISM extends its runtime Deno/V8 execution model (currently used by `prism_query`) into the event domain. Orchestration logic is defined in code inside the user's repository via a dedicated SDK.

**Example Implementation:**
```typescript
import { prism } from "@prism/sdk/hooks";

// Hook triggered on explicit graph state mutations
prism.onTaskActionable((task) => {
    if (task.executor.target_label === "cloud-agent") {
        // Evaluate side effect logic seamlessly
        prism.exec(`docker run -d my-agent-image --task ${task.id}`);
    }
});

// Hook triggered on temporal intervals
prism.onCron("0 2 * * *", () => {
    prism.mutate.coordination_transaction([
        // Dispatch "Continue As New" creation
    ]);
});
```

Because the PRISM daemon evaluates this TypeScript against the active active SQLite state, developers receive full IDE intellisense. The identical configuration file can execute locally on a laptop, or on an enterprise server monitoring a central remote.

---

## 4. Decentralized Event Locks (Event Tombstones)

### The Problem: The Thundering Herd
PRISM is a decentralized mesh. Three developers on laptops and two continuous integration servers might all be monitoring the identical `refs/prism/coordination/live` branch.
When a highly anticipated `Task` becomes `actionable`, all 5 daemons will evaluate the `.prism/hooks.ts` file concurrently. If the hook is `spawn-cloud-agent`, the swarm will incorrectly spin up 5 identical containers.

### The Solution: Shared Ref CAS Tombstones
PRISM does not rely on external cloud idempotency keys, nor does it pollute the primary `Plan/Task` DAG with transient event nodes. It uses the innate atomic protection of Git Compare-And-Swap (CAS) in an isolated namespace.

1. **Hash Generation:** When a hook condition matches, all 5 daemons deterministically hash the trigger (e.g., `evt_spawn_task_123_actionable`).
2. **The Race:** The daemons do not execute the side effect immediately. Instead, they attempt to commit and push a tiny JSON "tombstone" file to the Shared Refs tree:
   `refs/prism/coordination/live:.prism/state/events/processed/evt_spawn_task_123.json`
3. **The CAS Gate:** Git's atomic push guarantees strictly enforce that only one push can succeed.
4. **The Execution Winner:**
   - The 4 daemons whose git push gets **rejected** will automatically perform a fast-forward pull, see that `evt_spawn_task_123.json` now exists in the tree, and cleanly abort their local execution.
   - The 1 daemon whose push **succeeds** holds the absolute mathematical lock. It proceeds to safely execute the local webhook or shell script.

### Conclusion
This pattern operates entirely within PRISM's local-first, Git-dependent philosophy. It creates a bulletproof, distributed exactly-once execution lock without requiring external SaaS coordinators, Redis caches, or complex leader elections.
