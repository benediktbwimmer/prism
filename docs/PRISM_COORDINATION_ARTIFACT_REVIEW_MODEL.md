# PRISM Coordination Artifact And Review Model

Status: normative task, artifact, and review contract for coordination v2
Audience: coordination, query, MCP, UI, and workflow maintainers
Scope: task-scoped evidence, review-gate tasks, reopen and yield semantics, and upfront task
creation contracts for required artifacts and reviews

---

## 1. Summary

In coordination v2:

- `Task` remains the only claimable workflow unit
- `Artifact` is durable evidence produced by work, ideally as a pointer to git or external evidence
- `Review` is a verdict record attached to exactly one artifact
- review work is modeled as ordinary tasks with an explicit `reviewScope`
- plan completion remains graph-derived from plan and task state only

This model keeps workflow execution inside the canonical `Plan` and `Task` graph while preserving
artifact and review evidence as first-class coordination records.

## 2. Core Invariants

The system must preserve these rules:

1. The graph has only `Plan` and `Task` nodes.
2. Tasks are the only claimable entities.
3. Artifacts and reviews are not graph nodes.
4. A primitive review record always targets exactly one artifact.
5. A review task may aggregate many review records into one review pass.
6. Artifact and review requirements must be declared up front on task creation.
7. Shared refs remain the authoritative coordination substrate.
8. Local SQLite is a materialization and restart accelerator only.
9. Every review outcome that changes coordination state must do so atomically.

## 3. Why This Model

This contract separates three different concerns cleanly:

- workflow execution
  - which work exists, what depends on what, who is holding a lease
- produced evidence
  - what output or checkpoint a task created
- judgment over evidence
  - whether the produced evidence was approved, needs changes, or was rejected

If review work is modeled only as artifact metadata, it loses claimability and dependency behavior.
If review work is modeled only as graph tasks, it loses durable structured verdicts over evidence.
The v2 model keeps both:

- review work is a task
- review verdicts are attached review records over artifacts

## 4. Artifact Model

Artifacts are task-scoped evidence records.

### 4.1 Artifact Requirements

Tasks may declare one or more required artifacts up front. A task that requires an artifact cannot
complete until that artifact requirement is satisfied.

Artifact requirements should be pointer-oriented rather than payload-oriented.

Preferred evidence targets:

- git commit
- git range
- git ref
- external CI run or deployment id
- compact note or attestation when no stronger evidence source exists

Artifacts should not duplicate full code diffs that already live in git.

### 4.2 Artifact Requirement Shape

Conceptual task-create fields:

- `clientArtifactRequirementId`
- `kind`
- `minCount`
- `evidenceTypes`
- `staleAfterGraphChange`
- `requiredValidations`

Example:

```json
{
  "clientArtifactRequirementId": "impl_patch",
  "kind": "code_change",
  "minCount": 1,
  "evidenceTypes": ["git_commit", "git_range"],
  "staleAfterGraphChange": true,
  "requiredValidations": ["cargo test -p prism-mcp ssr_console::"]
}
```

## 5. Review Model

### 5.1 Review Requirements

Review requirements are declared up front. They define that some artifact requirement must
eventually be reviewed.

They are not themselves review records.

Conceptual review-requirement fields:

- `clientReviewRequirementId`
- `artifactRequirementRef`
- `allowedReviewerClasses`
- `minReviewCount`

Example:

```json
{
  "clientReviewRequirementId": "impl_patch_review",
  "artifactRequirementRef": "impl_patch",
  "allowedReviewerClasses": ["agent", "human"],
  "minReviewCount": 1
}
```

### 5.2 Primitive Review Records

A primitive review record is attached to exactly one artifact.

It records:

- the reviewed artifact
- the associated review requirement
- the review task that emitted it
- the verdict
- the reviewer identity and reviewer class
- a compact summary

Allowed verdicts:

- `approved`
- `changes_requested`
- `rejected`

The verdict is always allowed to be any of those values. Review policy does not predeclare a
required verdict. The verdict determines the next workflow transition.

## 6. Review Tasks

Review tasks are ordinary tasks. They are not a new graph entity type.

What makes them special is their task-local policy:

- `kind: review_gate`
- `reviewScope`

### 6.1 Review Scope

The review scope tells the review task which requested reviews it is responsible for resolving.

Conceptual fields:

- `requestedReviewRefs`
- `selection`
- `allTargetsRequired`

The common case is to point to explicit review requirements declared by upstream tasks.

Example:

```json
{
  "kind": "review_gate",
  "reviewScope": {
    "requestedReviewRefs": ["impl_patch_review", "tests_review"],
    "selection": "latest_non_superseded",
    "allTargetsRequired": true
  }
}
```

The query layer resolves this declared scope into concrete artifacts grouped by source task.

## 7. Upfront Task Creation Contract

The task-create surface must support declaring required artifacts and reviews up front, with client
ids used throughout one transaction.

### 7.1 Why Client Ids

Client ids should be used for:

- tasks
- artifact requirements
- review requirements

This gives:

- one readable transaction payload
- deterministic intra-transaction references
- easier plan authoring and debugging

### 7.2 Conceptual `task_create` Shape

```json
{
  "clientTaskId": "implement_sorting",
  "title": "Implement sorting",
  "dependsOn": [],
  "acceptance": [
    { "label": "Plans sort by created time", "anchors": [] }
  ],
  "requiredArtifacts": [
    {
      "clientArtifactRequirementId": "impl_patch",
      "kind": "code_change",
      "minCount": 1,
      "evidenceTypes": ["git_commit", "git_range"],
      "staleAfterGraphChange": true
    }
  ],
  "requiredReviews": [
    {
      "clientReviewRequirementId": "impl_patch_review",
      "artifactRequirementRef": "impl_patch",
      "allowedReviewerClasses": ["agent", "human"],
      "minReviewCount": 1
    }
  ]
}
```

Review task creation in the same transaction:

```json
{
  "clientTaskId": "final_review",
  "title": "Final review gate",
  "kind": "review_gate",
  "dependsOn": ["implement_sorting", "add_tests"],
  "reviewScope": {
    "requestedReviewRefs": ["impl_patch_review", "tests_review"],
    "selection": "latest_non_superseded",
    "allTargetsRequired": true
  }
}
```

## 8. Review Pass Semantics

A review task may emit many primitive review records during one review pass.

The review task state semantics are:

- while the reviewer is working the declared scope, the task is `InReview`
- each reviewed artifact immediately produces a durable review record
- the review task does not resolve on the first verdict
- the current pass resolves only once all required review targets for that pass have a verdict

When the pass resolves:

- if all required review targets are `approved`, the review task becomes `Completed`
- if at least one required review target is `changes_requested` or `rejected`, the review task
  releases its lease and becomes `Blocked`

This enables chunked review passes and prevents premature resolution after only one target has been
judged.

## 9. Atomic Review Outcome Rules

Review outcomes must drive the next graph transition atomically.

### 9.1 `approved`

Atomic effects:

- record the review
- update the reviewed artifact state
- if the pass is now fully satisfied, complete the review task

### 9.2 `changes_requested`

Atomic effects:

- record the review
- update the reviewed artifact state
- reopen exactly one existing task
- release the review task lease
- transition the review task to `Blocked` on the reopened task

This path is for small or moderate corrections to the same implementation intent.

### 9.3 `rejected`

Atomic effects:

- record the review
- update the reviewed artifact state
- create one or more explicit follow-up task objects
- wire their dependencies in the same transaction
- release the review task lease
- transition the review task to `Blocked` on the follow-up task set

This path is for cases where the current implementation should not simply continue as-is.

`rejected` must require the follow-up tasks and wiring in the review payload. If the graph update
cannot be expressed completely, the mutation must fail and the review task must remain claimable
later.

## 10. Reopen And Yield

### 10.1 Reopen

The system should support explicit `reopen_task`.

`reopen_task` is the coordination action that turns a previously completed implementation task back
into active graph work.

It should be used by the atomic `changes_requested` review path.

### 10.2 Yield

The system should also support explicit `yield_task` for all task kinds.

Yield means:

- release the current lease voluntarily
- leave durable checkpoint evidence behind
- transition the task back to `Ready` if it is still actionable

This is useful for:

- normal implementation tasks
- validation tasks
- review tasks with chunked passes

Yield must require a checkpoint, not necessarily an artifact.

A valid checkpoint may be:

- a new artifact
- a new review record
- a new outcome or validation record
- a structured yield summary or handoff note

If the runtime can prove that meaningful edits or review work happened, the yield mutation should
enforce stronger checkpoint requirements rather than relying only on instructions.

## 11. Query Surface Requirements

The query layer must expose enough structure that resumed work is never blind.

Minimum useful queries:

- `taskArtifacts(taskId)`
- `taskReviews(taskId)`
- `taskCheckpointSummary(taskId)`
- `taskReviewScope(taskId)`
- `taskReviewTargets(taskId)`
- `taskReviewStatus(taskId)`

The task brief and task detail surfaces should immediately surface:

- latest relevant review summaries
- unresolved review requirements
- current review scope status
- related artifact ids and evidence refs
- reopen or follow-up context when work was bounced back by review

## 12. Canonical Example

One transaction should be able to create all of this upfront:

```json
{
  "action": "coordination_transaction",
  "input": {
    "mutations": [
      {
        "action": "task_create",
        "input": {
          "clientTaskId": "implement_sorting",
          "title": "Implement sorting",
          "dependsOn": [],
          "requiredArtifacts": [
            {
              "clientArtifactRequirementId": "impl_patch",
              "kind": "code_change",
              "minCount": 1,
              "evidenceTypes": ["git_commit", "git_range"],
              "staleAfterGraphChange": true
            }
          ],
          "requiredReviews": [
            {
              "clientReviewRequirementId": "impl_patch_review",
              "artifactRequirementRef": "impl_patch",
              "allowedReviewerClasses": ["agent", "human"],
              "minReviewCount": 1
            }
          ]
        }
      },
      {
        "action": "task_create",
        "input": {
          "clientTaskId": "final_review",
          "title": "Final review gate",
          "kind": "review_gate",
          "dependsOn": ["implement_sorting"],
          "reviewScope": {
            "requestedReviewRefs": ["impl_patch_review"],
            "selection": "latest_non_superseded",
            "allTargetsRequired": true
          }
        }
      }
    ]
  }
}
```

Then later:

- implementation produces an artifact satisfying `impl_patch`
- review task resolves `impl_patch_review` to that artifact
- reviewer records one review record over that artifact
- if approved, the review task completes
- if changes are requested, the implementation task is reopened atomically
- if rejected, follow-up tasks are created atomically and the review task blocks on them

## 13. Non-Goals

This document does not define:

- exact Rust struct names
- exact MCP wire tags
- UI rendering details
- external review provider integration

It defines the behavioral contract that those surfaces must implement.
