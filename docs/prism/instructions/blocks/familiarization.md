## Familiarization

- Start with `prism://session` to confirm the active workspace root, task context, limits, and feature flags.
- Then inspect `prism://capabilities` to confirm the available query methods, resources, tools, and feature gates.
- Inspect `prism://vocab` before guessing enum spellings, action names, status values, edge kinds, or other closed vocabularies.
- In truncation-prone harnesses, prefer `prism://capabilities/{section}`, `prism://shape/tool/{toolName}`, `prism://example/tool/{toolName}`, `prism://shape/resource/{resourceKind}`, and `prism://example/resource/{resourceKind}` before reaching for larger schema payloads.
- Use `prism://tool-schemas` and `prism://schema/tool/{toolName}` when the task depends on exact MCP mutation or tool payload shapes, and prefer `prism://schema/tool/{toolName}/action/{action}` or `.../variant/{tag}` over the full union when you already know the target branch.
- Use `prism://recipe/tool/{toolName}/action/{action}` or `.../variant/{tag}` when the workflow is complex and you need the server-authored authoring path, common mistakes, or the minimum viable payload.
- Use `prism://api-reference` after the basic server shape is clear and you need the typed query surface or usage recipes.
