# PRISM Instruction Sets

Read this resource first after `prism://startup` reports `phase: ready`.

PRISM now exposes role-specific instruction sets. Choose the set that best matches the prompt you were launched with, then read `prism://instructions/{id}` before substantial work.

Selection rule:
- If the prompt is about picking up actionable tasks, implementing changes, or carrying claimed work to completion, load `execution`.
- If the prompt is about creating or refining a plan, decomposing work, or shaping dependencies and priority, load `planning`.
- If the prompt is about reviewing work, validating behavior, or identifying regressions and risks, load `review`.
- If the prompt is about repo-wide readiness, dispatch, task availability, or multi-agent handoff state, load `coordination`.
- If the prompt is about understanding an unfamiliar area, finding likely owners, or building context before a plan or implementation exists, load `exploration`.

Shared rule:
- Role resources already include the shared operating blocks they depend on. Read the chosen role resource directly instead of manually stitching together individual blocks.
- If your role changes materially during the session, read the new role resource before continuing.

{{INSTRUCTION_SET_INDEX}}
