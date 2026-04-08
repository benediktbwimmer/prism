# PRISM Knowledge Scope

Status: normative contract  
Audience: knowledge, runtime, query, MCP, CLI, UI, and future service maintainers  
Scope: scope semantics for published and draft knowledge

---

## 1. Goal

PRISM must define one explicit scope contract for knowledge.

This contract exists so that:

- repo-scoped knowledge and future project-scoped knowledge do not blur together
- publication and query surfaces can reason about scope consistently
- cross-repo evolution does not require rewriting basic knowledge semantics

## 2. Core invariants

Knowledge scope must preserve these rules:

1. Every published knowledge object has explicit scope.
2. Repo-scoped knowledge and project-scoped knowledge are distinct semantic scopes.
3. Draft knowledge may be local, but published knowledge scope must be explicit and durable.
4. Scope is identity-driven, not path-string folklore.

## 3. Minimum scopes

The contract must support:

- `repo_id`
- future `project_id`

Other local runtime scopes may exist operationally, but published knowledge scope should remain
intentionally small and durable.

## 4. Relationship to queries

Knowledge queries must allow scope-qualified reads and should surface when a result was selected
from:

- repo scope
- project scope
- a union or layered view if such a view is explicitly requested

## 5. Minimum implementation bar

This contract is considered implemented only when:

- published knowledge objects have stable explicit scope
- scope no longer depends on incidental storage location alone
