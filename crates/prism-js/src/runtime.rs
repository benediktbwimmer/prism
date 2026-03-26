pub fn runtime_prelude() -> &'static str {
    r#""use strict";

function __prismDecode(raw) {
  const envelope = JSON.parse(raw);
  if (!envelope.ok) {
    throw new Error(envelope.error);
  }
  return envelope.value;
}

function __prismHost(operation, args) {
  const payload = args === undefined ? "{}" : JSON.stringify(args);
  return __prismDecode(__prismHostCall(operation, payload));
}

function __prismNormalizeTarget(target) {
  if (target == null) {
    return null;
  }
  if (typeof target === "object" && target.id != null) {
    return target.id;
  }
  return target;
}

function __prismEnrichSymbol(raw) {
  if (raw == null) {
    return null;
  }

  return {
    ...raw,
    full() {
      return __prismHost("full", { id: this.id });
    },
    excerpt(options = {}) {
      return __prismHost("excerpt", {
        id: this.id,
        contextLines: options?.contextLines,
        maxLines: options?.maxLines,
        maxChars: options?.maxChars,
      });
    },
    relations() {
      return __prismEnrichRelations(__prismHost("relations", { id: this.id }));
    },
    callGraph(depth = 3) {
      return __prismEnrichSubgraph(__prismHost("callGraph", { id: this.id, depth }));
    },
    lineage() {
      return __prismEnrichLineage(__prismHost("lineage", { id: this.id }));
    },
  };
}

function __prismEnrichSymbols(values) {
  return Array.isArray(values) ? values.map(__prismEnrichSymbol) : [];
}

function __prismEnrichRelations(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    contains: __prismEnrichSymbols(raw.contains),
    callers: __prismEnrichSymbols(raw.callers),
    callees: __prismEnrichSymbols(raw.callees),
    references: __prismEnrichSymbols(raw.references),
    imports: __prismEnrichSymbols(raw.imports),
    implements: __prismEnrichSymbols(raw.implements),
    specifies: __prismEnrichSymbols(raw.specifies),
    specifiedBy: __prismEnrichSymbols(raw.specifiedBy),
    validates: __prismEnrichSymbols(raw.validates),
    validatedBy: __prismEnrichSymbols(raw.validatedBy),
    related: __prismEnrichSymbols(raw.related),
    relatedBy: __prismEnrichSymbols(raw.relatedBy),
  };
}

function __prismEnrichSubgraph(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    nodes: __prismEnrichSymbols(raw.nodes),
  };
}

function __prismEnrichLineage(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    current: __prismEnrichSymbol(raw.current),
  };
}

function __prismEnrichInsightCandidate(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    symbol: __prismEnrichSymbol(raw.symbol),
  };
}

function __prismEnrichInsightCandidates(values) {
  return Array.isArray(values) ? values.map(__prismEnrichInsightCandidate) : [];
}

function __prismEnrichSpecCluster(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    spec: __prismEnrichSymbol(raw.spec),
    implementations: __prismEnrichSymbols(raw.implementations),
    validations: __prismEnrichSymbols(raw.validations),
    related: __prismEnrichSymbols(raw.related),
    readPath: __prismEnrichInsightCandidates(raw.readPath),
    writePath: __prismEnrichInsightCandidates(raw.writePath),
    persistencePath: __prismEnrichInsightCandidates(raw.persistencePath),
    tests: __prismEnrichInsightCandidates(raw.tests),
  };
}

function __prismEnrichSpecDrift(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    spec: __prismEnrichSymbol(raw.spec),
    nextReads: __prismEnrichInsightCandidates(raw.nextReads),
    cluster: __prismEnrichSpecCluster(raw.cluster),
  };
}

function __prismEnrichReadContext(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    target: __prismEnrichSymbol(raw.target),
    directLinks: __prismEnrichSymbols(raw.directLinks),
    suggestedReads: __prismEnrichInsightCandidates(raw.suggestedReads),
    tests: __prismEnrichInsightCandidates(raw.tests),
  };
}

function __prismEnrichEditContext(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    target: __prismEnrichSymbol(raw.target),
    directLinks: __prismEnrichSymbols(raw.directLinks),
    suggestedReads: __prismEnrichInsightCandidates(raw.suggestedReads),
    writePaths: __prismEnrichInsightCandidates(raw.writePaths),
    tests: __prismEnrichInsightCandidates(raw.tests),
  };
}

function __prismNormalizeFocus(values) {
  if (!Array.isArray(values)) {
    return [];
  }
  return values
    .map(__prismNormalizeTarget)
    .filter((value) => value != null);
}

function __prismNormalizeAnchor(value) {
  if (value == null) {
    return null;
  }
  if (typeof value === "object" && value.id != null) {
    return {
      type: "node",
      crateName: value.id.crateName,
      path: value.id.path,
      kind: value.id.kind,
    };
  }
  if (typeof value === "object" && value.crateName != null && value.path != null) {
    return {
      type: "node",
      crateName: value.crateName,
      path: value.path,
      kind: value.kind,
    };
  }
  if (typeof value === "object" && value.Node != null) {
    const node = value.Node;
    return {
      type: "node",
      crateName: node.crateName ?? node.crate_name,
      path: node.path,
      kind: node.kind,
    };
  }
  if (typeof value === "object" && value.Lineage != null) {
    return {
      type: "lineage",
      lineageId: value.Lineage.lineageId ?? value.Lineage.lineage_id ?? value.Lineage,
    };
  }
  if (typeof value === "object" && value.File != null) {
    return {
      type: "file",
      fileId: value.File.fileId ?? value.File.file_id ?? value.File,
    };
  }
  if (typeof value === "object" && value.Kind != null) {
    return { type: "kind", kind: value.Kind.kind ?? value.Kind };
  }
  if (typeof value === "object" && typeof value.type === "string") {
    return value;
  }
  return null;
}

function __prismNormalizeAnchors(values) {
  const list = Array.isArray(values) ? values : [values];
  return list.map(__prismNormalizeAnchor).filter((value) => value != null);
}

function __prismCleanupGlobals() {
  for (const name of Object.getOwnPropertyNames(globalThis)) {
    if (__prismBaselineGlobals.includes(name)) {
      continue;
    }
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
    if (!descriptor || descriptor.configurable) {
      delete globalThis[name];
    }
  }
}

globalThis.prism = Object.freeze({
  symbol(query) {
    return __prismEnrichSymbol(__prismHost("symbol", { query }));
  },
  symbols(query) {
    return __prismEnrichSymbols(__prismHost("symbols", { query }));
  },
  search(query, options = {}) {
    return __prismEnrichSymbols(
      __prismHost("search", {
        query,
        limit: options?.limit,
        kind: options?.kind,
        path: options?.path,
        strategy: options?.strategy,
        ownerKind: options?.ownerKind ?? options?.owner_kind,
        includeInferred: options?.includeInferred ?? options?.include_inferred,
      })
    );
  },
  entrypoints() {
    return __prismEnrichSymbols(__prismHost("entrypoints", {}));
  },
  plan(planId) {
    return __prismHost("plan", { planId });
  },
  task(taskId) {
    return __prismHost("coordinationTask", { taskId });
  },
  readyTasks(planId) {
    return __prismHost("readyTasks", { planId });
  },
  claims(target) {
    return __prismHost("claims", { anchors: __prismNormalizeAnchors(target) });
  },
  conflicts(target) {
    return __prismHost("conflicts", { anchors: __prismNormalizeAnchors(target) });
  },
  blockers(taskId) {
    return __prismHost("blockers", { taskId });
  },
  pendingReviews(planId) {
    return __prismHost("pendingReviews", planId == null ? {} : { planId });
  },
  artifacts(taskId) {
    return __prismHost("artifacts", { taskId });
  },
  policyViolations(input = {}) {
    return __prismHost("policyViolations", {
      planId: input?.planId ?? input?.plan_id,
      taskId: input?.taskId ?? input?.task_id,
      limit: input?.limit,
    });
  },
  taskBlastRadius(taskId) {
    return __prismHost("taskBlastRadius", { taskId });
  },
  taskValidationRecipe(taskId) {
    return __prismHost("taskValidationRecipe", { taskId });
  },
  taskRisk(taskId) {
    return __prismHost("taskRisk", { taskId });
  },
  artifactRisk(artifactId) {
    return __prismHost("artifactRisk", { artifactId });
  },
  taskIntent(taskId) {
    return __prismHost("taskIntent", { taskId });
  },
  coordinationInbox(planId) {
    return {
      readyTasks: prism.readyTasks(planId),
      pendingReviews: prism.pendingReviews(planId),
    };
  },
  taskContext(taskId) {
    const task = prism.task(taskId);
    const target = task?.anchors ?? [];
    return {
      task,
      blockers: prism.blockers(taskId),
      artifacts: prism.artifacts(taskId),
      claims: target.length > 0 ? prism.claims(target) : [],
      conflicts: target.length > 0 ? prism.conflicts(target) : [],
      blastRadius: prism.taskBlastRadius(taskId),
      validationRecipe: prism.taskValidationRecipe(taskId),
      risk: prism.taskRisk(taskId),
    };
  },
  claimPreview(input) {
    const conflicts = prism.simulateClaim(input);
    return {
      conflicts,
      blocked: conflicts.some((conflict) => conflict.severity === "Block"),
      warnings: conflicts.filter((conflict) => conflict.severity !== "Info"),
    };
  },
  simulateClaim(input) {
    return __prismHost("simulateClaim", {
      anchors: __prismNormalizeAnchors(input?.anchors ?? input?.anchor ?? []),
      capability: input?.capability,
      mode: input?.mode,
      taskId: input?.taskId ?? input?.task_id,
    });
  },
  lineage(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismEnrichLineage(__prismHost("lineage", { id }));
  },
  coChangeNeighbors(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismHost("coChangeNeighbors", { id });
  },
  relatedFailures(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismHost("relatedFailures", { id });
  },
  blastRadius(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismHost("blastRadius", { id });
  },
  validationRecipe(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismHost("validationRecipe", { id });
  },
  readContext(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismEnrichReadContext(__prismHost("readContext", { id }));
  },
  editContext(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismEnrichEditContext(__prismHost("editContext", { id }));
  },
  nextReads(target, options = {}) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismEnrichInsightCandidates(
      __prismHost("nextReads", {
        id,
        limit: options?.limit,
      })
    );
  },
  whereUsed(target, options = {}) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismEnrichSymbols(
      __prismHost("whereUsed", {
        id,
        mode: options?.mode,
        limit: options?.limit,
      })
    );
  },
  entrypointsFor(target, options = {}) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismEnrichSymbols(
      __prismHost("entrypointsFor", {
        id,
        limit: options?.limit,
      })
    );
  },
  specFor(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismEnrichSymbols(__prismHost("specFor", { id }));
  },
  implementationFor(target, options = {}) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismEnrichSymbols(
      __prismHost("implementationFor", {
        id,
        mode: options?.mode,
        ownerKind: options?.ownerKind ?? options?.owner_kind,
      })
    );
  },
  owners(target, options = {}) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismEnrichInsightCandidates(
      __prismHost("owners", {
        id,
        kind: options?.kind,
        limit: options?.limit,
      })
    );
  },
  driftCandidates(limit) {
    return __prismHost("driftCandidates", limit == null ? {} : { limit });
  },
  specCluster(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismEnrichSpecCluster(__prismHost("specCluster", { id }));
  },
  explainDrift(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return null;
    }
    return __prismEnrichSpecDrift(__prismHost("explainDrift", { id }));
  },
  resumeTask(taskId) {
    return __prismHost("resumeTask", { taskId });
  },
  taskJournal(taskId, options = {}) {
    return __prismHost("taskJournal", {
      taskId,
      eventLimit: options.eventLimit ?? options.event_limit,
      memoryLimit: options.memoryLimit ?? options.memory_limit,
    });
  },
  memory: Object.freeze({
    recall(options = {}) {
      return __prismHost("memoryRecall", {
        focus: __prismNormalizeFocus(options.focus),
        text: options.text,
        limit: options.limit,
        kinds: options.kinds,
        since: options.since,
      });
    },
    outcomes(options = {}) {
      return __prismHost("memoryOutcomes", {
        focus: __prismNormalizeFocus(options.focus),
        taskId: options.taskId,
        kinds: options.kinds,
        result: options.result,
        actor: options.actor,
        since: options.since,
        limit: options.limit,
      });
    },
  }),
  curator: Object.freeze({
    jobs(options = {}) {
      return __prismHost("curatorJobs", options);
    },
    job(id) {
      if (typeof id !== "string" || id.length === 0) {
        return null;
      }
      return __prismHost("curatorJob", { job_id: id });
    },
  }),
  diagnostics() {
    return __prismHost("diagnostics", {});
  },
});

const __prismBaselineGlobals = Object.getOwnPropertyNames(globalThis);
"#
}
