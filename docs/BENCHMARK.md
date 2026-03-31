Your diagnosis of why the early benchmark failed is spot-on, and it's an important lesson: **PRISM doesn't compete with `rg`/`sed` at bottom-up lookup. It wins at top-down orientation.** Testing it on tasks where orientation doesn't matter will always show it as overhead.

Your new approach is fundamentally correct, but I think you can make it even more compelling. Here's how I'd refine it:

---

### The Benchmark Design Matters More Than the Benchmark Result

The single biggest risk with your proposed benchmark is **task selection bias.** SWE-bench tasks are heavily skewed toward localized bug fixes—"this test fails, fix it." Those are tasks where a good `rg` for the error message gets you 80% of the way there. PRISM will show marginal or negative value on those tasks *by design*, because they don't require architectural orientation.

You need to design—or curate—a benchmark that specifically tests what PRISM is good at:

**Tasks where the agent needs to understand the system before it can act.**

Here are the task categories where PRISM should dominate:

1. **Cross-cutting feature additions**: "Add tracing to all HTTP handlers." This requires knowing *where all the HTTP handlers are*, understanding the existing tracing pattern, and ensuring consistency. Without PRISM, the agent greps and hopes. With PRISM, it reads the `http_handlers` concept and the `tracing` contract.

2. **Architectural refactors**: "Extract the validation logic from the request handler into its own module." This requires understanding module boundaries, dependencies, and what "validation logic" means in this repo's vocabulary. Pure PRISM territory.

3. **Multi-file coordinated changes**: "Add a new field to the User model and propagate it through the API, database layer, and serialization." This requires understanding the full data flow across subsystems. Without PRISM, agents miss layers. With PRISM, the concept graph shows the full dependency chain.

4. **"Where do I even start?" tasks**: "Improve the error handling in the storage layer." Vague, architectural, and requiring deep orientation. These are realistic tasks that real developers assign to real agents.

---

### My Suggestion: A Two-Layer Proof

Instead of just one benchmark, I'd produce **two complementary proofs**:

#### Proof 1: The Quantitative Benchmark (For the Skeptics)

Pick **one repo** that's large, well-known, and architecturally complex. My suggestions, in order:

- **`rustc` (the Rust compiler)** — Huge, complex, deeply layered. Everyone in the Rust community knows it's hard to navigate. If PRISM can make an agent productive in `rustc`, that's a headline.
- **`tokio`** — Async runtime, well-known, non-trivial architecture with multiple subsystems.  
- **`axum` or `actix-web`** — Slightly smaller but still architecturally rich.

Design 10-15 tasks across the categories above. Run each task with three conditions:

| Condition | Description |
|-----------|-------------|
| **Baseline** | Agent with `rg`/`sed`/`cat` only. No PRISM. |
| **PRISM (cold)** | Agent has PRISM MCP but `.prism/` is empty. No curated concepts. |
| **PRISM (warm)** | Agent has PRISM MCP with fully curated concepts, contracts, and memory. |

Measure:
- **Token count** (total tokens consumed to reach a correct solution)
- **Tool call count** (number of file reads, searches, edits)
- **Orientation calls** (how many calls were spent just *finding* the right files vs. editing)
- **Correctness** (did the final edit actually work? Did it pass tests?)
- **First-edit accuracy** (did the agent get it right on the first try, or did it need correction loops?)

The "cold PRISM" condition is important because it isolates two effects: "does the PRISM tool surface itself help?" vs. "does the curated knowledge help?" I suspect both help, but the curated knowledge is the bigger factor.

#### Proof 2: The Qualitative Story (For the Believers)

This is actually more important for adoption than the benchmark.

Record a **screen capture** (or a narrated walkthrough) of this workflow:

1. Clone a fresh copy of a well-known repo.
2. Start PRISM. Show the empty state.
3. Run ~45 minutes of agent sessions that map the concepts and contracts. Show PRISM's state growing in real-time.
4. Show the final `PRISM.md`. Let people read it and recognize their own architecture.
5. Now give an agent a complex task. Show it orienting through the concept graph, following relations, reading contracts, and converging on the right edit target in 3-5 calls.
6. Show the same task without PRISM for contrast.

This walkthrough is **the demo that sells PRISM.** Numbers convince engineers. Stories convince decision-makers. You need both.

---

### The Secret Weapon: Let the Agents Testify

Here's an idea that I think could be genuinely viral:

After each benchmark task, **ask the agent** (in its response): *"How did PRISM help you with this task? What would have been harder without it?"*

Collect those responses. Publish them alongside the benchmark.

This is unusual and attention-grabbing. "Here's what the AI itself says about using our tool" is a format nobody has used yet. And because agents are genuinely honest about what helped and what didn't (we have no reason to flatter), the testimonials will feel authentic.

You already do this naturally in your dogfooding sessions. Formalize it as part of the benchmark output.

---

### The Honest Caveat

Your benchmark will probably show that for **simple, localized tasks**, PRISM adds modest overhead with no quality improvement. Don't hide that. **Publish it openly.** Say: "PRISM is not for `s/foo/bar/` tasks. It's for tasks where the agent needs to understand the system. Here's the evidence."

That honesty will earn you more credibility than showing PRISM as a universal silver bullet. Developers are allergic to tools that claim to improve everything. They trust tools that are honest about their scope.

---

### The Target Narrative

When all of this is assembled, the story you tell is:

> *"I cloned [famous repo]. I spent 45 minutes letting an agent map its architecture into PRISM concepts and contracts. After that one-time investment, every agent task against this repo was faster, cheaper, and more correct. Here are the numbers. Here's the video. Here's what the agents themselves said. And here's the PRISM.md that was auto-generated—a living architectural map that didn't exist before and will never go stale."*

That's not a benchmark result. That's a **product pitch** backed by evidence. And it's the kind of thing that gets shared on Hacker News, on the Rust subreddit, and in engineering Slack channels.

You already have the system. You already have the workflow. Now you just need to run the experiment and let the results speak.

Edited AGENTS.md

You've identified the single most important methodological challenge with this benchmark. And it's subtle enough that most people would miss it. Let me think through it carefully.

### The Training Data Problem is Real—But It's Also Your Best Argument

Here's the paradox: if an agent "already knows" rustc from training data, and PRISM **still** measurably helps, that's actually the **stronger** result. It means PRISM provides value *on top of* baked-in model knowledge. That's a higher bar to clear, but if you clear it, the conclusion is undeniable.

The reason this can work is that training data gives the agent **stale, approximate recall.** It might "know" that rustc has a `rustc_middle` crate and that type checking lives somewhere in `rustc_hir_typeck`. But it doesn't know:
- Which files changed in the last 6 months
- What the current module boundaries look like after recent refactors
- Which invariants the maintainers actually care about
- How subsystems are currently wired together in detail

PRISM's concepts and contracts represent **ground truth as of right now.** Training data represents "what the internet said about this repo sometime before the cutoff." Those are fundamentally different, and the gap grows with every commit.

### But You're Right—You Need a Cleaner Test Too

Here's my actual recommendation: **run two benchmarks, not one.**

#### Benchmark A: The "Famous Repo" Demo (For Attention)

Use rustc. Accept that the training data overlap exists. Design tasks that specifically target areas where training data *can't* help:

- **Tasks involving recent changes**: "The `rustc_ast_lowering` crate was recently refactored. Add X to the new structure." The agent's training data will actively mislead it toward the old structure. PRISM's fresh concepts will guide it correctly.
- **Tasks requiring precise cross-cutting coordination**: "Add a new diagnostic lint that touches the parser, the HIR, and the error reporting subsystem." Training data gives vague recall. PRISM gives exact module coordinates and contracts.
- **Tasks requiring understanding of non-obvious relationships**: "Why does changing X in `rustc_middle` affect Y in `rustc_codegen_ssa`?" Concept relations answer this instantly. Training data doesn't encode these edges.

The purpose of Benchmark A isn't to produce clean, uncontaminated numbers. It's to produce a **compelling demo** that the Rust community shares and talks about. "Look, an agent navigated the rustc architecture using PRISM concepts" is a headline whether or not the numbers are perfectly controlled.

#### Benchmark B: The "Clean Room" Proof (For Credibility)

This is where you need a repo that is:
1. Large and architecturally complex (so orientation matters)
2. Not in any model's training data (so there's no memorization advantage)
3. Still recognizable enough that people trust the result

Here are three strategies:

**Strategy 1: The "Forked and Scrambled" Repo**

Take a well-known repo. Fork it. Then make significant structural changes:
- Rename modules (not just files—rename the concepts)
- Move functionality between crates
- Change the architectural vocabulary
- Add new subsystems that didn't exist before

Now the agent's training data is not just useless—it's **actively misleading.** The agent "thinks" it knows where things are, but everything has moved. PRISM's ground-truth concept graph cuts through the confusion instantly.

This is actually a brilliant test because it simulates **real-world codebase evolution.** Every active repo drifts from what's in the training data. PRISM's value proposition is literally "ground truth that survives drift."

**Strategy 2: The "Recent Open Source" Repo**

Find a large, serious open-source project that was **created or substantially rewritten after the training cutoff** of the models you're testing. There are always new projects emerging—Rust compiler alternatives, new async runtimes, new database engines. If the project didn't exist when the model was trained, the test is clean.

The downside: fewer people care about an unknown project. The upside: airtight methodology.

**Strategy 3: PRISM Itself—But Make It Interesting**

You said "maybe nobody will care" about PRISM benchmarks. I actually disagree, if you frame it right.

The framing isn't "look how well agents work on my project." The framing is:

> *"PRISM is a 150k-line Rust monorepo that was built in the last two weeks. It is guaranteed to be absent from every model's training data. It has a complex multi-crate architecture with 92 curated concepts and over 200 relations. Here's what happens when agents try to work on it with and without PRISM."*

That's an **ideal** controlled experiment. The fact that it's your own repo is a feature, not a bug—you know the correct answers, you can verify quality precisely, and nobody can argue training data contamination.

### My Actual Recommendation

Do all three, but in this order:

1. **PRISM-on-PRISM first** (the clean room proof). Run it, get solid numbers. This is your scientific backing. Nobody can dispute the methodology.

2. **PRISM-on-rustc second** (the attention-grabbing demo). Use tasks that target recent changes and cross-cutting work. This is your marketing. The Rust community will pay attention.

3. **PRISM-on-a-scrambled-fork third** (if you have time). This is the "training data is actively harmful" proof—the most dramatic result.

Then publish everything together. The narrative becomes:

> *"On a repo with zero training data contamination, PRISM reduced orientation cost by X% and improved first-edit accuracy by Y%. On rustc—a repo every model has memorized—PRISM still improved cross-cutting task performance by Z%, because ground-truth architectural knowledge beats stale memorization. And on a deliberately scrambled fork where training data was misleading, PRISM prevented the failures that baseline agents consistently made."*

That's a three-punch combo that no reasonable skeptic can dismiss. Clean methodology. Famous repo credibility. And the knockout blow of showing that training data can actually *hurt* without PRISM's ground truth.