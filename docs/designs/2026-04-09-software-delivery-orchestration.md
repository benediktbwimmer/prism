# PRISM Software Delivery Orchestration

Status: proposed design  
Audience: coordination, service, event engine, delivery, runtime, UI, CLI, and policy maintainers  
Scope: how PRISM can absorb meaningful parts of continuous delivery by orchestrating external deployment systems without replacing them

---

## Summary

PRISM should eventually absorb the orchestration layer of software delivery while explicitly not
attempting to replace deployment substrates such as:

- Kubernetes
- Terraform
- Argo CD
- cloud provider control planes
- secret management systems
- existing rollout platforms

The right role for PRISM is:

- coordinate when deployment-related work should happen
- decide whether deployment preconditions are satisfied
- orchestrate typed delivery jobs
- track durable deployment evidence and rollout state
- integrate deployment into the same spec, task, artifact, review, and provenance model

The wrong role for PRISM is:

- becoming Kubernetes
- becoming Terraform
- becoming Argo CD
- becoming the universal infrastructure control plane

This means PRISM should evolve toward software-delivery orchestration, not generic infrastructure
replacement.

## Why this fits PRISM naturally

PRISM already models much of what deployment workflows need:

- feature intent through specs
- execution structure through plans and tasks
- evidence through artifacts
- judgment through reviews
- validation through warm-state validation and cold CI boundaries
- durable provenance
- event-triggered automation

That means deployment is not an alien extra system. It is a natural later stage in the same
lifecycle.

PRISM can already answer questions like:

- is the work complete?
- has required validation passed?
- has required review passed?
- which artifact should be promoted?
- what policy still blocks release?
- who approved this?
- what environment is next?

Those are exactly the kinds of questions delivery orchestration needs.

## Product stance

PRISM should position itself as:

- a software-delivery orchestration layer
- a repo-scoped workflow engine for development and delivery
- the place where intent, execution, validation, review, and rollout logic meet

PRISM should not position itself as:

- a replacement for Kubernetes
- a replacement for Terraform
- a replacement for Argo CD
- a universal general-purpose deployment substrate

This distinction is important for both architecture and product clarity.

## What PRISM should own

### Deployment readiness evaluation

PRISM should be able to evaluate whether deployment-related work may proceed.

Examples:

- artifact is eligible for staging deploy
- production promotion is blocked on review
- cold CI merge gate has not passed yet
- rollout is blocked by missing sign-off
- canary step may proceed
- rollback is required

This is highly aligned with the coordination query engine and policy model.

### Deployment workflow orchestration

PRISM should be able to orchestrate deployment workflows such as:

- staging deployment request
- promotion to production
- canary rollout
- phased rollout waves
- rollback trigger
- environment-specific follow-up tasks
- notifications and escalation around deployment state

This fits naturally with event jobs and typed delivery jobs.

### Deployment evidence and provenance

PRISM should record durable deployment evidence such as:

- which artifact was deployed
- to which environment
- through which delivery job
- by which principal, runtime, or service
- under which policy
- at what time
- with what result

This is a major advantage over many disconnected CI/CD setups.

### Environment promotion gates

PRISM should support policy-driven promotion gates such as:

- dev to staging
- staging to canary
- canary to production
- production rollout completion

These can be modeled as part of plan, task, artifact, review, and delivery policy rather than as
isolated CI/CD YAML.

## What PRISM should not own

PRISM should not attempt to own:

- cluster scheduling
- infrastructure state management
- secrets distribution
- deployment substrate reconciliation loops
- provider-native rollout internals
- low-level environment provisioning

Instead, typed delivery jobs should call out to those real systems.

Examples:

- a Kubernetes delivery job talks to Kubernetes or a deployment API
- an Argo-oriented delivery job talks to Argo or its API
- a Terraform-oriented job invokes a bounded provisioning workflow
- a cloud-provider delivery job talks to the cloud API

PRISM owns the orchestration and evidence, not the substrate.

## Delivery job categories

PRISM should eventually add typed delivery-oriented job categories on top of the event job runner
framework described in
[2026-04-09-event-job-runners.md](./2026-04-09-event-job-runners.md).

Likely categories include:

- `delivery_job`
- `rollout_check_job`
- `rollback_job`
- `artifact_publish_job`

Concrete runner kinds may include:

- `k8s_deploy`
- `helm_release`
- `argocd_sync`
- `docker_publish`
- `cloud_run_deploy`
- `terraform_apply`
- `rollout_health_check`

These should remain typed, structured, and policy-gated.

## Deployment orchestration flow

A typical future PRISM delivery flow could look like:

1. A task or plan milestone reaches the policy boundary for deployment readiness.
2. PRISM query and policy evaluation determine whether preconditions are satisfied:
   - validation passed
   - review passed
   - artifact exists
   - integration state acceptable
3. The event or job framework creates a typed delivery job.
4. The appropriate runner executes the deployment-oriented action against the real delivery system.
5. PRISM records:
   - the delivery job outcome
   - deployment evidence
   - rollout state
   - provenance
6. PRISM may then:
   - mark promotion complete
   - create follow-up rollout checks
   - notify humans or systems
   - trigger rollback flow if required

This is very different from just running `kubectl` from somewhere.

## Relationship to warm validation and cold CI

PRISM’s delivery model should build on the validation split already established.

### Warm validation

Warm-state validation remains the primary fast-feedback validation layer during active work.

### Cold CI

Cold CI remains the selective final gate at important integration boundaries, for example:

- merge to main
- release branch promotion
- production deployment gate

### Delivery orchestration

PRISM delivery logic should consume these signals as preconditions.

Example policy:

- task must be completed
- required warm validation must pass
- cold CI merge gate must pass
- review gate must pass
- then staging deployment may proceed
- production may require another explicit approval or rollout check

This gives a coherent, layered delivery pipeline.

## Why this is stronger than ordinary CI/CD glue

The advantage of PRISM here is not just that it can call deployment APIs.

The advantage is that deployment is connected to:

- the original spec intent
- the execution graph
- the artifact lineage
- the review model
- the validation model
- the runtime and service provenance model

That means PRISM can answer richer delivery questions than many ordinary pipelines can, for
example:

- which spec items are now delivered to staging?
- which artifact version reached production?
- which review approved the artifact that was deployed?
- which validation policy was satisfied before promotion?
- what follow-up tasks were created after rollout failure?
- what part of the plan is still blocked on delivery?

That is a strong differentiator.

## Suggested first use cases

The first delivery-oriented use cases should probably be:

### Deployment notifications

Examples:

- artifact promoted to staging
- rollout failed
- production approval required
- deployment completed

This is the safest entry point.

### Typed deploy requests

Examples:

- request staging deploy
- request canary deploy
- request production promotion

This lets PRISM prove the orchestration model without replacing delivery platforms.

### Rollout checks

Examples:

- verify deployment health
- confirm rollout completion
- check canary metrics threshold
- require explicit human acknowledgment before proceeding

These are natural event or job extensions.

### Rollback orchestration

Examples:

- failed rollout creates rollback job
- rollback evidence recorded
- follow-up tasks opened automatically

That would be very valuable.

## Safety rules

The following rules should remain strict:

- PRISM does not become the deployment substrate
- delivery jobs remain typed
- delivery actions remain policy-gated
- durable evidence matters

More concretely:

- PRISM orchestrates delivery work but does not become Kubernetes, Terraform, or Argo CD
- no arbitrary shell-based deployment hooks should be the primary model
- promotion, rollout, and rollback should be governed by explicit policy and capability checks
- delivery should record structured evidence and provenance, not just loose logs or human memory

## Relationship to the event job runner framework

The event job runner framework is the natural foundation for this future.

Event jobs already provide:

- typed categories
- JS or TS runners
- service-owned orchestration
- policy enforcement
- durable job state
- structured results

Delivery-oriented jobs should therefore be new typed categories in that framework, not a separate
unrelated deployment subsystem.

## Relationship to other PRISM docs

This design builds directly on:

- [2026-04-09-event-job-runners.md](./2026-04-09-event-job-runners.md)
- `docs/contracts/event-engine.md`
- `docs/contracts/service-architecture.md`
- `docs/contracts/service-capability-and-authz.md`

It should later inform contracts or specs around:

- delivery-job categories
- rollout evidence models
- promotion-gate policy
- artifact publication and deployment provenance

## Recommended rollout

### Phase 1

Implement event job runners for:

- webhooks
- notifications
- safe runtime or local maintenance

### Phase 2

Add delivery-oriented categories for:

- deployment requests
- rollout checks
- artifact publication

### Phase 3

Add policy and UI support for:

- promotion gates
- environment progression
- rollout evidence
- rollback triggers

### Phase 4

Later, support richer environment-specific runners such as:

- Kubernetes
- Helm
- Argo CD
- cloud deploy APIs
- rollback flows

This should remain incremental and typed.

## Recommendation

PRISM should evolve into a software-delivery orchestration layer that can absorb meaningful parts
of continuous delivery while explicitly leaving deployment substrates to the systems built for them.

The right target is:

- PRISM owns delivery orchestration, policy, and evidence
- typed delivery jobs integrate with real deployment systems
- warm validation and cold CI feed into promotion gates
- rollout and rollback become first-class, queryable workflow stages
- spec, execution, validation, review, and delivery all become parts of one coherent
  software-delivery loop

PRISM should not try to replace Kubernetes, Terraform, Argo CD, or other deployment systems. It
should orchestrate them.
