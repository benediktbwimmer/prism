## Read Strategy

- Prefer checking `prism://vocab` before guessing enum spellings or mutation action names.
- Prefer checking `prism.tool("...")`, `prism://tool-schemas`, and `prism://schema/tool/{toolName}` before hand-writing non-trivial mutation payloads.
- Prefer compact top-level tools and typed query views over ad hoc query snippets whenever they can express the task.
- Prefer PRISM-native file inspection and bounded context retrieval when they can replace multiple shell reads with one staged call, especially `prism_locate`, `prism_gather`, `prism_open`, `prism_workset`, `prism_expand`, `prism.file(path).read(...)`, `prism.file(path).around(...)`, and `prism.searchText(...)`.
- Prefer compact PRISM tools and bounded PRISM-native reads over manual line-window shell reads such as `sed` and `cat` when the work can be expressed in one staged PRISM flow.
- Targeted `rg` is acceptable for exact-text narrowing, test-name lookup, or fast filename discrimination before returning to PRISM for the actual read or edit context.
- Keep shell reads as a fallback for raw bytes, command output, or cases where PRISM cannot yet express the needed inspection precisely.
