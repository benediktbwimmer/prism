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
      __prismHost("search", Object.assign({ query }, options))
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
  specFor(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismEnrichSymbols(__prismHost("specFor", { id }));
  },
  implementationFor(target) {
    const id = __prismNormalizeTarget(target);
    if (id == null) {
      return [];
    }
    return __prismEnrichSymbols(__prismHost("implementationFor", { id }));
  },
  driftCandidates(limit) {
    return __prismHost("driftCandidates", limit == null ? {} : { limit });
  },
  resumeTask(taskId) {
    return __prismHost("resumeTask", { taskId });
  },
  memory: Object.freeze({
    recall(options = {}) {
      return __prismHost("memoryRecall", {
        focus: __prismNormalizeFocus(options.focus),
        text: options.text,
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
