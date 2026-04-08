# PRISM Shared Scope And Identity

Status: normative cross-layer contract  
Audience: coordination, knowledge, cognition, runtime, query, MCP, CLI, UI, auth, and future service maintainers  
Scope: stable shared identities and scope vocabulary across PRISM layers

---

## 1. Goal

PRISM must define one shared scope and identity vocabulary used across coordination, knowledge, and
cognition.

This contract exists so that:

- layers do not invent incompatible identity models
- repo, project, runtime, and principal scope remain stable across contracts
- cross-repo growth has one identity spine

Canonical ownership:

- this document defines the shared vocabulary only
- it does not define full actor and trust semantics
- it does not define runtime descriptor publication or discovery behavior

## 2. Minimum shared identities

The shared minimum identity vocabulary must include:

- `repo_id`
- future `project_id`
- `worktree_id`
- `runtime_id`
- `principal_id`

Additional identities may exist, but these should remain the common base.

## 3. Core invariants

The shared identity model must preserve these rules:

1. Identities are logical and durable, not path-derived accidents.
2. Scope must be recorded explicitly when it matters.
3. Cross-layer references should prefer stable logical ids over local path strings.

## 4. Relationship to specialized contracts

This contract is the shared vocabulary layer.

Specialized contracts refine it for their own domains:

- [identity-model.md](./identity-model.md)
- [runtime-identity-and-descriptors.md](./runtime-identity-and-descriptors.md)
- [knowledge-scope.md](./knowledge-scope.md)
- [coordination-authority-store.md](./coordination-authority-store.md)

## 5. Minimum implementation bar

This contract is considered implemented only when:

- the common ids are used consistently across the major layer contracts
- cross-repo growth no longer depends on ad hoc local path conventions
