use std::sync::OnceLock;

use crate::runtime_option_keys_js_object;

pub fn runtime_prelude() -> &'static str {
    static PRELUDE: OnceLock<String> = OnceLock::new();
    PRELUDE.get_or_init(|| {
        RUNTIME_PRELUDE_TEMPLATE.replace(
            "__PRISM_OPTION_KEYS_OBJECT__",
            runtime_option_keys_js_object(),
        )
    })
}

const RUNTIME_PRELUDE_TEMPLATE: &str = r#""use strict";

function __prismDecode(raw) {
  const envelope = JSON.parse(raw);
  if (!envelope.ok) {
    if (envelope.queryError != null) {
      throw new Error(`__PRISM_QUERY_USER_ERROR__${JSON.stringify(envelope.queryError)}`);
    }
    throw new Error(envelope.error);
  }
  return envelope.value;
}

function __prismHost(operation, args) {
  const payload = args === undefined ? "{}" : JSON.stringify(args);
  return __prismDecode(__prismHostCall(operation, payload));
}

function __prismThrowQueryUserError(summary, message, data = {}) {
  throw new Error(
    `__PRISM_QUERY_USER_ERROR__${JSON.stringify({ summary, message, data })}`
  );
}

function __prismNormalizedApiToken(value) {
  return typeof value === "string" ? value.replace(/[_-]/g, "").toLowerCase() : "";
}

function __prismLevenshtein(left, right) {
  if (left === right) {
    return 0;
  }
  if (left.length === 0) {
    return right.length;
  }
  if (right.length === 0) {
    return left.length;
  }
  const previous = new Array(right.length + 1);
  const current = new Array(right.length + 1);
  for (let index = 0; index <= right.length; index += 1) {
    previous[index] = index;
  }
  for (let row = 0; row < left.length; row += 1) {
    current[0] = row + 1;
    for (let column = 0; column < right.length; column += 1) {
      const cost = left[row] === right[column] ? 0 : 1;
      current[column + 1] = Math.min(
        current[column] + 1,
        previous[column + 1] + 1,
        previous[column] + cost
      );
    }
    for (let index = 0; index <= right.length; index += 1) {
      previous[index] = current[index];
    }
  }
  return previous[right.length];
}

function __prismSuggestApiToken(value, candidates) {
  const normalized = __prismNormalizedApiToken(value);
  if (!normalized) {
    return null;
  }
  let bestCandidate = null;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (const candidate of candidates) {
    if (typeof candidate !== "string" || candidate.length === 0) {
      continue;
    }
    const candidateNormalized = __prismNormalizedApiToken(candidate);
    if (candidateNormalized === normalized) {
      return candidate;
    }
    const distance = __prismLevenshtein(normalized, candidateNormalized);
    if (distance < bestDistance) {
      bestDistance = distance;
      bestCandidate = candidate;
    }
  }
  if (bestCandidate == null) {
    return null;
  }
  const threshold = Math.max(2, Math.floor(normalized.length / 3));
  return bestDistance <= threshold ? bestCandidate : null;
}

function __prismValidateRecordShape(methodPath, value, argumentName, allowedKeys) {
  if (value == null) {
    return {};
  }
  if (typeof value !== "object" || Array.isArray(value)) {
    __prismThrowQueryUserError(
      "prism_query arguments invalid",
      `prism_query arguments invalid for \`${methodPath}\`: \`${argumentName}\` must be an object when provided.\nHint: Pass a plain object with the documented keys, or omit \`${argumentName}\` entirely.`,
      {
        code: "query_invalid_argument",
        category: "invalid_argument",
        method: methodPath,
        error: `\`${argumentName}\` must be an object when provided.`,
        nextAction: `Pass a plain object for \`${argumentName}\`, or omit it entirely. Check \`prism://api-reference\` for the exact shape.`,
      }
    );
  }
  const unknownKeys = Object.keys(value).filter((key) => !allowedKeys.includes(key));
  if (unknownKeys.length === 0) {
    return value;
  }
  const didYouMean = {};
  for (const key of unknownKeys) {
    const suggestion = __prismSuggestApiToken(key, allowedKeys);
    if (suggestion != null) {
      didYouMean[key] = suggestion;
    }
  }
  const unknownSummary = unknownKeys.map((key) => `\`${key}\``).join(", ");
  const suggestionSummary = Object.entries(didYouMean)
    .map(([key, suggestion]) => `\`${key}\` -> \`${suggestion}\``)
    .join(", ");
  const hint =
    suggestionSummary.length > 0
      ? `Use the documented key spelling instead (${suggestionSummary}) and retry.`
      : `Use only documented keys for \`${methodPath}\` and retry.`;
  __prismThrowQueryUserError(
    "prism_query arguments invalid",
    `prism_query arguments invalid for \`${methodPath}\`: unknown ${unknownKeys.length === 1 ? "key" : "keys"} ${unknownSummary} in \`${argumentName}\`.\nHint: ${hint} Check \`prism://api-reference\` for the exact shape.`,
    {
      code: "query_invalid_argument",
      category: "invalid_argument",
      method: methodPath,
      invalidKeys: unknownKeys,
      didYouMean,
      error: `Unknown ${unknownKeys.length === 1 ? "key" : "keys"} ${unknownSummary} in \`${argumentName}\`.`,
      nextAction: `${hint} Check \`prism://api-reference\` for the exact shape.`,
    }
  );
}

function __prismValidateOptions(methodPath, options, allowedKeys) {
  return __prismValidateRecordShape(methodPath, options, "options", allowedKeys);
}

function __prismPickOptions(options, allowedKeys) {
  if (options == null || typeof options !== "object" || Array.isArray(options)) {
    return {};
  }
  const picked = {};
  for (const key of allowedKeys) {
    if (Object.prototype.hasOwnProperty.call(options, key)) {
      picked[key] = options[key];
    }
  }
  return picked;
}

function __prismNormalizeTarget(target) {
  if (target == null) {
    return null;
  }
  if (typeof target === "object") {
    if (target.id != null) {
      return target.id;
    }
    if (
      typeof target.crateName === "string" &&
      typeof target.path === "string" &&
      typeof target.kind === "string"
    ) {
      return target;
    }
    return null;
  }
  return target;
}

function __prismNormalizeTargetPayload(target) {
  if (target == null) {
    return null;
  }
  const lineageId =
    typeof target === "object" && typeof target.lineageId === "string" && target.lineageId.trim() !== ""
      ? target.lineageId
      : undefined;
  const id = __prismNormalizeTarget(target);
  if (id == null && lineageId == null) {
    return null;
  }
  return lineageId == null ? { id } : id == null ? { lineageId } : { id, lineageId };
}

function __prismBundleSeedTarget(target) {
  if (target == null || typeof target !== "object") {
    return target;
  }
  if (target.topResult != null) {
    return target.topResult;
  }
  if (target.target != null) {
    return target.target;
  }
  return target;
}

function __prismDiscoveryFromBundle(target) {
  if (target == null || typeof target !== "object") {
    return null;
  }
  if (target.discovery != null && typeof target.discovery === "object" && target.discovery.target != null) {
    return target.discovery;
  }
  if (
    target.target != null &&
    target.readContext != null &&
    target.editContext != null &&
    target.validationContext != null &&
    target.recentChangeContext != null
  ) {
    return target;
  }
  return null;
}

function __prismIncludeDiscovery(options = {}) {
  return options?.includeDiscovery === true;
}

function __prismSuggestedReadLimit(options = {}) {
  return options?.suggestedReadLimit ?? options?.suggested_read_limit ?? 5;
}

const __prismOptionKeys = __PRISM_OPTION_KEYS_OBJECT__;

function __prismTextSearchSemanticQuery(query, options = {}) {
  if (typeof options?.semanticQuery === "string" && options.semanticQuery.trim() !== "") {
    return options.semanticQuery;
  }
  return options?.regex === true ? null : query;
}

function __prismDiagnosticCodes(diagnostics) {
  return Array.isArray(diagnostics)
    ? diagnostics
        .map((diagnostic) => diagnostic?.code)
        .filter((code) => typeof code === "string" && code.length > 0)
    : [];
}

function __prismWithLocalDiagnostics(run) {
  const before = prism.diagnostics();
  const beforeCount = Array.isArray(before) ? before.length : 0;
  const value = run();
  const after = prism.diagnostics();
  return {
    value,
    diagnostics: Array.isArray(after) ? after.slice(beforeCount) : [],
  };
}

function __prismBundleSummary(kind, resultCount, diagnostics) {
  const diagnosticCodes = __prismDiagnosticCodes(diagnostics);
  return {
    kind,
    resultCount,
    empty: resultCount === 0,
    truncated: diagnosticCodes.includes("result_truncated"),
    ambiguous:
      diagnosticCodes.includes("ambiguous_search") || diagnosticCodes.includes("ambiguous_symbol"),
    diagnosticCodes,
  };
}

function __prismResolveSuggestedReads(target, discovery, readContext, options = {}) {
  if (Array.isArray(discovery?.suggestedReads) && discovery.suggestedReads.length > 0) {
    return discovery.suggestedReads;
  }
  if (Array.isArray(readContext?.suggestedReads) && readContext.suggestedReads.length > 0) {
    return readContext.suggestedReads;
  }
  return target != null ? prism.nextReads(target, { limit: __prismSuggestedReadLimit(options) }) : [];
}

function __prismNormalizePath(path) {
  if (typeof path !== "string" || path.trim() === "") {
    throw new Error("path must be a non-empty string");
  }
  return path;
}

function __prismEnrichSymbol(raw) {
  if (raw == null) {
    return null;
  }

  return {
    ...raw,
    full() {
      return __prismHost("full", __prismNormalizeTargetPayload(this));
    },
    excerpt(options = {}) {
      return __prismHost("excerpt", {
        ...__prismNormalizeTargetPayload(this),
        contextLines: options?.contextLines,
        maxLines: options?.maxLines,
        maxChars: options?.maxChars,
      });
    },
    editSlice(options = {}) {
      return __prismHost("editSlice", {
        ...__prismNormalizeTargetPayload(this),
        beforeLines: options?.beforeLines,
        afterLines: options?.afterLines,
        maxLines: options?.maxLines,
        maxChars: options?.maxChars,
      });
    },
    relations() {
      return __prismEnrichRelations(__prismHost("relations", __prismNormalizeTargetPayload(this)));
    },
    callGraph(depth = 3) {
      return __prismEnrichSubgraph(
        __prismHost("callGraph", { ...__prismNormalizeTargetPayload(this), depth })
      );
    },
    lineage() {
      return __prismEnrichLineage(__prismHost("lineage", __prismNormalizeTargetPayload(this)));
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

function __prismEnrichConceptDecode(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    primary: __prismEnrichSymbol(raw.primary),
    members: __prismEnrichSymbols(raw.members),
    supportingReads: __prismEnrichSymbols(raw.supportingReads),
    likelyTests: __prismEnrichSymbols(raw.likelyTests),
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

function __prismEnrichFocusedBlock(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    symbol: __prismEnrichSymbol(raw.symbol),
  };
}

function __prismEnrichFocusedBlocks(values) {
  return Array.isArray(values) ? values.map(__prismEnrichFocusedBlock) : [];
}

function __prismEnrichReadContext(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    target: __prismEnrichSymbol(raw.target),
    targetBlock: __prismEnrichFocusedBlock(raw.targetBlock),
    directLinks: __prismEnrichSymbols(raw.directLinks),
    directLinkBlocks: __prismEnrichFocusedBlocks(raw.directLinkBlocks),
    suggestedReads: __prismEnrichInsightCandidates(raw.suggestedReads),
    tests: __prismEnrichInsightCandidates(raw.tests),
    testBlocks: __prismEnrichFocusedBlocks(raw.testBlocks),
  };
}

function __prismEnrichEditContext(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    target: __prismEnrichSymbol(raw.target),
    targetBlock: __prismEnrichFocusedBlock(raw.targetBlock),
    directLinks: __prismEnrichSymbols(raw.directLinks),
    directLinkBlocks: __prismEnrichFocusedBlocks(raw.directLinkBlocks),
    suggestedReads: __prismEnrichInsightCandidates(raw.suggestedReads),
    writePaths: __prismEnrichInsightCandidates(raw.writePaths),
    writePathBlocks: __prismEnrichFocusedBlocks(raw.writePathBlocks),
    tests: __prismEnrichInsightCandidates(raw.tests),
    testBlocks: __prismEnrichFocusedBlocks(raw.testBlocks),
  };
}

function __prismEnrichValidationContext(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    target: __prismEnrichSymbol(raw.target),
    tests: __prismEnrichInsightCandidates(raw.tests),
    targetBlock: __prismEnrichFocusedBlock(raw.targetBlock),
    testBlocks: __prismEnrichFocusedBlocks(raw.testBlocks),
  };
}

function __prismEnrichRecentChangeContext(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    target: __prismEnrichSymbol(raw.target),
    lineage: __prismEnrichLineage(raw.lineage),
  };
}

function __prismEnrichDiscoveryBundle(raw) {
  if (raw == null) {
    return raw;
  }
  return {
    ...raw,
    target: __prismEnrichSymbol(raw.target),
    suggestedReads: __prismEnrichInsightCandidates(raw.suggestedReads),
    readContext: __prismEnrichReadContext(raw.readContext),
    editContext: __prismEnrichEditContext(raw.editContext),
    validationContext: __prismEnrichValidationContext(raw.validationContext),
    recentChangeContext: __prismEnrichRecentChangeContext(raw.recentChangeContext),
    entrypoints: __prismEnrichSymbols(raw.entrypoints),
    whereUsedDirect: __prismEnrichSymbols(raw.whereUsedDirect),
    whereUsedBehavioral: __prismEnrichSymbols(raw.whereUsedBehavioral),
    relations: __prismEnrichRelations(raw.relations),
    specCluster: __prismEnrichSpecCluster(raw.specCluster),
    specDrift: __prismEnrichSpecDrift(raw.specDrift),
    lineage: __prismEnrichLineage(raw.lineage),
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
      path: value.File.path,
      fileId: value.File.fileId ?? value.File.file_id ?? (typeof value.File === "number" ? value.File : undefined),
    };
  }
  if (typeof value === "object" && value.Kind != null) {
    return { type: "kind", kind: value.Kind.kind ?? value.Kind };
  }
  if (typeof value === "object" && typeof value.type === "string") {
    if (value.type === "file") {
      return {
        ...value,
        path: value.path,
        fileId: value.fileId ?? value.file_id,
      };
    }
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

function __prismFile(path) {
  const filePath = __prismNormalizePath(path);
  return Object.freeze({
    path: filePath,
    read(options = {}) {
      options = __prismValidateOptions("prism.file(path).read", options, __prismOptionKeys.fileRead);
      return __prismHost("fileRead", {
        path: filePath,
        startLine: options?.startLine,
        endLine: options?.endLine,
        maxChars: options?.maxChars,
      });
    },
    around(options = {}) {
      options = __prismValidateOptions(
        "prism.file(path).around",
        options,
        __prismOptionKeys.fileAround
      );
      return __prismHost("fileAround", {
        path: filePath,
        line: options?.line,
        before: options?.before ?? options?.beforeLines,
        after: options?.after ?? options?.afterLines,
        maxChars: options?.maxChars,
      });
    },
  });
}

function __prismSymbolBundle(query, options = {}) {
  const scoped = __prismWithLocalDiagnostics(() => {
    const hasQuery = typeof query === "string" && query.trim() !== "";
    const directLookup = hasQuery
      ? __prismWithLocalDiagnostics(() => prism.symbol(query))
      : { value: null, diagnostics: [] };
    const directResult = directLookup.value;
    const directDiagnostics = directLookup.diagnostics;
    const needsCandidates =
      hasQuery &&
      (directResult == null ||
        __prismDiagnosticCodes(directDiagnostics).includes("ambiguous_symbol"));
    const candidates = needsCandidates
      ? prism.search(query, {
          limit: options?.limit ?? options?.candidateLimit ?? options?.candidate_limit ?? 5,
          kind: options?.kind,
          path: options?.path,
          module: options?.module,
          strategy: options?.strategy,
          preferCallableCode:
            options?.preferCallableCode ?? options?.prefer_callable_code,
          preferEditableTargets:
            options?.preferEditableTargets ?? options?.prefer_editable_targets,
          preferBehavioralOwners:
            options?.preferBehavioralOwners ?? options?.prefer_behavioral_owners,
          ownerKind: options?.ownerKind ?? options?.owner_kind,
          includeInferred: options?.includeInferred ?? options?.include_inferred,
        })
      : directResult != null
        ? [directResult]
        : [];
    const result = directResult ?? (candidates.length > 0 ? candidates[0] : null);
    const discovery =
      result != null && __prismIncludeDiscovery(options) ? prism.discovery(result) : null;
    const readContext = result ? discovery?.readContext ?? prism.readContext(result) : null;
    return {
      query,
      result,
      candidates,
      discovery,
      focusedBlock: result ? prism.focusedBlock(result) : null,
      readContext,
      suggestedReads: __prismResolveSuggestedReads(result, discovery, readContext, options),
    };
  });
  return {
    ...scoped.value,
    summary: __prismBundleSummary("symbol", scoped.value.candidates.length, scoped.diagnostics),
    diagnostics: scoped.diagnostics,
  };
}

let __prismDynamicViewsLoaded = false;
const __prismDynamicViews = new Map();

function __prismDynamicViewMethod(name) {
  if (!__prismDynamicViews.has(name)) {
    __prismDynamicViews.set(name, function(input = {}) {
      return __prismHost(`__queryView:${name}`, __prismNormalizeDynamicViewInput(input));
    });
  }
  return __prismDynamicViews.get(name);
}

function __prismNormalizeDynamicViewInput(input = {}) {
  if (input == null || typeof input !== "object" || Array.isArray(input)) {
    return input;
  }
  const normalized = { ...input };
  if (Object.prototype.hasOwnProperty.call(normalized, "target")) {
    normalized.target = __prismNormalizeTargetPayload(normalized.target);
  }
  return normalized;
}

function __prismLoadDynamicViews() {
  if (__prismDynamicViewsLoaded) {
    return __prismDynamicViews;
  }
  const views = __prismHost("__queryViews", {});
  if (Array.isArray(views)) {
    for (const view of views) {
      if (view && typeof view.name === "string" && view.name.length > 0) {
        __prismDynamicViewMethod(view.name);
      }
    }
  }
  __prismDynamicViewsLoaded = true;
  return __prismDynamicViews;
}

const __prismBase = Object.freeze({
  symbol(query) {
    return __prismEnrichSymbol(__prismHost("symbol", { query }));
  },
  symbolBundle: __prismSymbolBundle,
  symbols(query) {
    return __prismEnrichSymbols(__prismHost("symbols", { query }));
  },
  search(query, options = {}) {
    options = __prismValidateOptions("prism.search", options, __prismOptionKeys.search);
    return __prismEnrichSymbols(
      __prismHost("search", {
        query,
        limit: options?.limit,
        kind: options?.kind,
        path: options?.path,
        module: options?.module,
        taskId: options?.taskId ?? options?.task_id,
        pathMode: options?.pathMode ?? options?.path_mode,
        strategy: options?.strategy,
        structuredPath: options?.structuredPath ?? options?.structured_path,
        topLevelOnly: options?.topLevelOnly ?? options?.top_level_only,
        preferCallableCode:
          options?.preferCallableCode ?? options?.prefer_callable_code,
        preferEditableTargets:
          options?.preferEditableTargets ?? options?.prefer_editable_targets,
        preferBehavioralOwners:
          options?.preferBehavioralOwners ?? options?.prefer_behavioral_owners,
        ownerKind: options?.ownerKind ?? options?.owner_kind,
        includeInferred: options?.includeInferred ?? options?.include_inferred,
      })
    );
  },
  concepts(query, options = {}) {
    options = __prismValidateOptions("prism.concepts", options, __prismOptionKeys.concept);
    return __prismHost("concepts", {
      query,
      limit: options?.limit,
      verbosity: options?.verbosity ?? "summary",
      includeBindingMetadata:
        options?.includeBindingMetadata ?? options?.include_binding_metadata,
    });
  },
  concept(query, options = {}) {
    options = __prismValidateOptions("prism.concept", options, __prismOptionKeys.concept);
    return __prismHost("concept", {
      query,
      limit: 1,
      verbosity: options?.verbosity ?? "standard",
      includeBindingMetadata:
        options?.includeBindingMetadata ?? options?.include_binding_metadata,
    });
  },
  conceptByHandle(handle, options = {}) {
    options = __prismValidateOptions(
      "prism.conceptByHandle",
      options,
      __prismOptionKeys.concept
    );
    return __prismHost("conceptByHandle", {
      handle,
      verbosity: options?.verbosity ?? "standard",
      includeBindingMetadata:
        options?.includeBindingMetadata ?? options?.include_binding_metadata,
    });
  },
  contract(query) {
    return __prismHost("contract", { query });
  },
  contracts(options = {}) {
    options = __prismValidateOptions("prism.contracts", options, __prismOptionKeys.contracts);
    return __prismHost("contracts", {
      status: options?.status,
      scope: options?.scope,
      contains: options?.contains,
      kind: options?.kind,
      limit: options?.limit,
    });
  },
  contractsFor(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return [];
    }
    return __prismHost("contractsFor", targetPayload);
  },
  conceptRelations(handle) {
    return __prismHost("conceptRelations", { handle });
  },
  decodeConcept(input) {
    if (typeof input === "string") {
      return __prismEnrichConceptDecode(
        __prismHost("decodeConcept", {
          query: input,
          lens: "open",
          verbosity: "standard",
        })
      );
    }
    input = __prismValidateRecordShape(
      "prism.decodeConcept",
      input,
      "input",
      ["handle", "query", "lens", "verbosity", "includeBindingMetadata", "include_binding_metadata"]
    );
    return __prismEnrichConceptDecode(
      __prismHost("decodeConcept", {
        handle: input?.handle,
        query: input?.query,
        lens: input?.lens ?? "open",
        verbosity: input?.verbosity ?? "standard",
        includeBindingMetadata:
          input?.includeBindingMetadata ?? input?.include_binding_metadata,
      })
    );
  },
  searchText(query, options = {}) {
    options = __prismValidateOptions("prism.searchText", options, __prismOptionKeys.searchText);
    return __prismHost("searchText", {
      query,
      regex: options?.regex,
      caseSensitive: options?.caseSensitive ?? options?.case_sensitive,
      path: options?.path,
      glob: options?.glob,
      limit: options?.limit,
      contextLines: options?.contextLines ?? options?.context_lines,
    });
  },
  tools() {
    return __prismHost("tools", {});
  },
  tool(name) {
    return __prismHost("tool", { name });
  },
  validateToolInput(name, input) {
    return __prismHost("validateToolInput", { name, input });
  },
  entrypoints() {
    return __prismEnrichSymbols(__prismHost("entrypoints", {}));
  },
  file(path) {
    return __prismFile(path);
  },
  plans(options = {}) {
    options = __prismValidateOptions("prism.plans", options, __prismOptionKeys.plans);
    return __prismHost("plans", {
      status: options?.status,
      scope: options?.scope,
      contains: options?.contains,
      limit: options?.limit,
    });
  },
  plan(planId) {
    return __prismHost("plan", { planId });
  },
  planGraph(planId) {
    return __prismHost("planGraph", { planId });
  },
  planProjectionAt(planId, at) {
    return __prismHost("planProjectionAt", { planId, at });
  },
  planProjectionDiff(planId, from, to) {
    return __prismHost("planProjectionDiff", { planId, from, to });
  },
  planExecution(planId) {
    return __prismHost("planExecution", { planId });
  },
  planReadyNodes(planId) {
    return __prismHost("planReadyNodes", { planId });
  },
  planNodeBlockers(planId, nodeId) {
    return __prismHost("planNodeBlockers", { planId, nodeId });
  },
  planSummary(planId) {
    return __prismHost("planSummary", { planId });
  },
  planNext(planId, limit) {
    return __prismHost("planNext", limit == null ? { planId } : { planId, limit });
  },
  portfolioNext(limit) {
    return __prismHost("portfolioNext", limit == null ? {} : { limit });
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
    input = __prismValidateRecordShape(
      "prism.policyViolations",
      input,
      "input",
      __prismOptionKeys.policyViolations
    );
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
  _workflowExecution(overlays) {
    return (overlays ?? []).filter((overlay) => {
      if (!overlay || typeof overlay !== "object") return false;
      return Boolean(
        overlay.pendingHandoffTo ??
          overlay.effectiveAssignee ??
          overlay.awaitingHandoffFrom ??
          overlay.gitExecution
      );
    });
  },
  taskIntent(taskId) {
    return __prismHost("taskIntent", { taskId });
  },
  coordinationInbox(planId) {
    const plan = prism.plan(planId);
    const planGraph = prism.planGraph(planId);
    return {
      plan,
      planGraph,
      planExecution: prism._workflowExecution(prism.planExecution(planId)),
      planSummary: prism.planSummary(planId),
      planNext: prism.planNext(planId),
      readyTasks: prism.readyTasks(planId),
      pendingReviews: prism.pendingReviews(planId),
    };
  },
  taskContext(taskId) {
    const task = prism.task(taskId);
    const planGraph = task ? prism.planGraph(task.planId) : null;
    const planExecution = task
      ? prism._workflowExecution(prism.planExecution(task.planId))
      : [];
    const taskNode = planGraph?.nodes.find((node) => node.id === taskId) ?? null;
    const taskExecution =
      task && planGraph
        ? planExecution.find((overlay) => overlay.nodeId === taskId) ?? null
        : null;
    const target = task?.anchors ?? [];
    return {
      task,
      taskNode,
      taskExecution,
      planGraph,
      planSummary: task ? prism.planSummary(task.planId) : null,
      planNext: task ? prism.planNext(task.planId) : [],
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
    input = __prismValidateRecordShape(
      "prism.claimPreview",
      input,
      "input",
      __prismOptionKeys.claimPreview
    );
    const conflicts = prism.simulateClaim(input);
    return {
      conflicts,
      blocked: conflicts.some((conflict) => conflict.severity === "Block"),
      warnings: conflicts.filter((conflict) => conflict.severity !== "Info"),
    };
  },
  simulateClaim(input) {
    input = __prismValidateRecordShape(
      "prism.simulateClaim",
      input,
      "input",
      __prismOptionKeys.claimPreview
    );
    return __prismHost("simulateClaim", {
      anchors: __prismNormalizeAnchors(input?.anchors ?? input?.anchor ?? []),
      capability: input?.capability,
      mode: input?.mode,
      taskId: input?.taskId ?? input?.task_id,
    });
  },
  full(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismHost("full", targetPayload);
  },
  excerpt(target, options = {}) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    options = __prismValidateOptions("prism.excerpt", options, __prismOptionKeys.excerpt);
    return __prismHost("excerpt", {
      ...targetPayload,
      contextLines: options?.contextLines,
      maxLines: options?.maxLines,
      maxChars: options?.maxChars,
    });
  },
  editSlice(target, options = {}) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    options = __prismValidateOptions("prism.editSlice", options, __prismOptionKeys.editSlice);
    return __prismHost("editSlice", {
      ...targetPayload,
      beforeLines: options?.beforeLines,
      afterLines: options?.afterLines,
      maxLines: options?.maxLines,
      maxChars: options?.maxChars,
    });
  },
  focusedBlock(target, options = {}) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    options = __prismValidateOptions(
      "prism.focusedBlock",
      options,
      __prismOptionKeys.editSlice
    );
    return __prismEnrichFocusedBlock(
      __prismHost("focusedBlock", {
        ...targetPayload,
        beforeLines: options?.beforeLines,
        afterLines: options?.afterLines,
        maxLines: options?.maxLines,
        maxChars: options?.maxChars,
      })
    );
  },
  lineage(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismEnrichLineage(__prismHost("lineage", targetPayload));
  },
  coChangeNeighbors(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return [];
    }
    return __prismHost("coChangeNeighbors", targetPayload);
  },
  relatedFailures(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return [];
    }
    return __prismHost("relatedFailures", targetPayload);
  },
  blastRadius(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismHost("blastRadius", targetPayload);
  },
  validationRecipe(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismHost("validationRecipe", targetPayload);
  },
  readContext(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismEnrichReadContext(__prismHost("readContext", targetPayload));
  },
  editContext(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismEnrichEditContext(__prismHost("editContext", targetPayload));
  },
  validationContext(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismEnrichValidationContext(__prismHost("validationContext", targetPayload));
  },
  recentChangeContext(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismEnrichRecentChangeContext(__prismHost("recentChangeContext", targetPayload));
  },
  discovery(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismEnrichDiscoveryBundle(__prismHost("discoveryBundle", targetPayload));
  },
  searchBundle(query, options = {}) {
    options = __prismValidateOptions(
      "prism.searchBundle",
      options,
      __prismOptionKeys.searchBundle
    );
    const scoped = __prismWithLocalDiagnostics(() => {
      const results = prism.search(
        query,
        __prismPickOptions(options, __prismOptionKeys.search)
      );
      const topResult = Array.isArray(results) && results.length > 0 ? results[0] : null;
      const discovery =
        topResult != null && __prismIncludeDiscovery(options) ? prism.discovery(topResult) : null;
      const readContext =
        topResult ? discovery?.readContext ?? prism.readContext(topResult) : null;
      return {
        query,
        results,
        topResult,
        discovery,
        focusedBlock: topResult ? prism.focusedBlock(topResult) : null,
        readContext,
        suggestedReads: __prismResolveSuggestedReads(topResult, discovery, readContext, options),
        validationContext:
          topResult ? discovery?.validationContext ?? prism.validationContext(topResult) : null,
        recentChangeContext:
          topResult
            ? discovery?.recentChangeContext ?? prism.recentChangeContext(topResult)
            : null,
      };
    });
    return {
      ...scoped.value,
      summary: __prismBundleSummary("search", scoped.value.results.length, scoped.diagnostics),
      diagnostics: scoped.diagnostics,
    };
  },
  textSearchBundle(query, options = {}) {
    options = __prismValidateOptions(
      "prism.textSearchBundle",
      options,
      __prismOptionKeys.textSearchBundle
    );
    const scoped = __prismWithLocalDiagnostics(() => {
      const matches = prism.searchText(
        query,
        __prismPickOptions(options, __prismOptionKeys.searchText)
      );
      const topMatch = Array.isArray(matches) && matches.length > 0 ? matches[0] : null;
      const rawContext =
        topMatch != null
          ? prism.file(topMatch.path).around({
              line: topMatch.location.startLine,
              before: options?.aroundBefore,
              after: options?.aroundAfter,
              maxChars: options?.aroundMaxChars,
            })
          : null;
      const semanticQuery =
        topMatch != null ? __prismTextSearchSemanticQuery(query, options) : null;
      const semanticResults =
        topMatch != null && semanticQuery != null
          ? prism.search(semanticQuery, {
              limit: options?.semanticLimit,
              path: topMatch.path,
              kind: options?.semanticKind,
              preferCallableCode:
                options?.preferCallableCode ?? options?.prefer_callable_code,
              preferEditableTargets:
                options?.preferEditableTargets ?? options?.prefer_editable_targets,
              preferBehavioralOwners:
                options?.preferBehavioralOwners ?? options?.prefer_behavioral_owners,
              ownerKind: options?.ownerKind ?? options?.owner_kind,
              strategy: options?.strategy,
              includeInferred: options?.includeInferred ?? options?.include_inferred,
            })
          : [];
      const topSymbol =
        Array.isArray(semanticResults) && semanticResults.length > 0 ? semanticResults[0] : null;
      const discovery =
        topSymbol != null && __prismIncludeDiscovery(options) ? prism.discovery(topSymbol) : null;
      const readContext =
        topSymbol ? discovery?.readContext ?? prism.readContext(topSymbol) : null;
      return {
        query,
        matches,
        topMatch,
        rawContext,
        semanticQuery,
        semanticResults,
        topSymbol,
        discovery,
        focusedBlock: topSymbol ? prism.focusedBlock(topSymbol) : null,
        readContext,
        suggestedReads: __prismResolveSuggestedReads(topSymbol, discovery, readContext, options),
      };
    });
    return {
      ...scoped.value,
      summary: __prismBundleSummary("text_search", scoped.value.matches.length, scoped.diagnostics),
      diagnostics: scoped.diagnostics,
    };
  },
  targetBundle(target, options = {}) {
    options = __prismValidateOptions(
      "prism.targetBundle",
      options,
      __prismOptionKeys.targetBundle
    );
    const scoped = __prismWithLocalDiagnostics(() => {
      const providedDiscovery = __prismDiscoveryFromBundle(target);
      const targetPayload = __prismNormalizeTargetPayload(__prismBundleSeedTarget(target));
      if (targetPayload == null) {
        return null;
      }
      const discovery =
        providedDiscovery ??
        (__prismIncludeDiscovery(options) ? prism.discovery(targetPayload) : null);
      const focusedBlock = prism.focusedBlock(targetPayload);
      const editContext = discovery?.editContext ?? prism.editContext(targetPayload);
      const readContext = discovery?.readContext ?? prism.readContext(targetPayload);
      const targetSymbol = discovery?.target ?? focusedBlock?.symbol ?? null;
      if (targetSymbol == null || editContext == null) {
        return null;
      }
      return {
        target: targetSymbol,
        discovery,
        focusedBlock,
        diff: prism.diffFor(
          targetPayload,
          __prismPickOptions(options, __prismOptionKeys.diffFor)
        ),
        editContext,
        readContext,
        suggestedReads: __prismResolveSuggestedReads(targetPayload, discovery, readContext, options),
        likelyTests: editContext?.testBlocks ?? [],
      };
    });
    if (scoped.value == null) {
      return null;
    }
    return {
      ...scoped.value,
      summary: __prismBundleSummary("target", 1, scoped.diagnostics),
      diagnostics: scoped.diagnostics,
    };
  },
  nextReads(target, options = {}) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return [];
    }
    options = __prismValidateOptions("prism.nextReads", options, __prismOptionKeys.nextReads);
    return __prismEnrichInsightCandidates(
      __prismHost("nextReads", {
        ...targetPayload,
        limit: options?.limit,
      })
    );
  },
  whereUsed(target, options = {}) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return [];
    }
    options = __prismValidateOptions("prism.whereUsed", options, __prismOptionKeys.whereUsed);
    return __prismEnrichSymbols(
      __prismHost("whereUsed", {
        ...targetPayload,
        mode: options?.mode,
        limit: options?.limit,
      })
    );
  },
  entrypointsFor(target, options = {}) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return [];
    }
    options = __prismValidateOptions(
      "prism.entrypointsFor",
      options,
      __prismOptionKeys.nextReads
    );
    return __prismEnrichSymbols(
      __prismHost("entrypointsFor", {
        ...targetPayload,
        limit: options?.limit,
      })
    );
  },
  specFor(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return [];
    }
    return __prismEnrichSymbols(__prismHost("specFor", targetPayload));
  },
  implementationFor(target, options = {}) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return [];
    }
    options = __prismValidateOptions(
      "prism.implementationFor",
      options,
      __prismOptionKeys.implementationFor
    );
    return __prismEnrichSymbols(
      __prismHost("implementationFor", {
        ...targetPayload,
        mode: options?.mode,
        ownerKind: options?.ownerKind ?? options?.owner_kind,
      })
    );
  },
  owners(target, options = {}) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return [];
    }
    options = __prismValidateOptions("prism.owners", options, __prismOptionKeys.owners);
    return __prismEnrichInsightCandidates(
      __prismHost("owners", {
        ...targetPayload,
        kind: options?.kind,
        limit: options?.limit,
      })
    );
  },
  driftCandidates(limit) {
    return __prismHost("driftCandidates", limit == null ? {} : { limit });
  },
  specCluster(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismEnrichSpecCluster(__prismHost("specCluster", targetPayload));
  },
  explainDrift(target) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return null;
    }
    return __prismEnrichSpecDrift(__prismHost("explainDrift", targetPayload));
  },
  resumeTask(taskId) {
    return __prismHost("resumeTask", { taskId });
  },
  taskJournal(taskId, options = {}) {
    options = __prismValidateOptions(
      "prism.taskJournal",
      options,
      __prismOptionKeys.taskJournal
    );
    return __prismHost("taskJournal", {
      taskId,
      eventLimit: options.eventLimit ?? options.event_limit,
      memoryLimit: options.memoryLimit ?? options.memory_limit,
    });
  },
  changedFiles(options = {}) {
    options = __prismValidateOptions(
      "prism.changedFiles",
      options,
      __prismOptionKeys.changedFiles
    );
    return __prismHost("changedFiles", {
      since: options?.since,
      limit: options?.limit,
      taskId: options?.taskId ?? options?.task_id,
      path: options?.path,
    });
  },
  changedSymbols(path, options = {}) {
    options = __prismValidateOptions(
      "prism.changedSymbols",
      options,
      __prismOptionKeys.changedSymbols
    );
    return __prismHost("changedSymbols", {
      path: __prismNormalizePath(path),
      since: options?.since,
      limit: options?.limit,
      taskId: options?.taskId ?? options?.task_id,
    });
  },
  recentPatches(options = {}) {
    options = __prismValidateOptions(
      "prism.recentPatches",
      options,
      __prismOptionKeys.recentPatches
    );
    return __prismHost("recentPatches", {
      target: __prismNormalizeTarget(options?.target),
      since: options?.since,
      limit: options?.limit,
      taskId: options?.taskId ?? options?.task_id,
      path: options?.path,
    });
  },
  diffFor(target, options = {}) {
    const targetPayload = __prismNormalizeTargetPayload(target);
    if (targetPayload == null) {
      return [];
    }
    options = __prismValidateOptions("prism.diffFor", options, __prismOptionKeys.diffFor);
    return __prismHost("diffFor", {
      ...targetPayload,
      since: options?.since,
      limit: options?.limit,
      taskId: options?.taskId ?? options?.task_id,
    });
  },
  taskChanges(taskId, options = {}) {
    options = __prismValidateOptions(
      "prism.taskChanges",
      options,
      __prismOptionKeys.taskChanges
    );
    return __prismHost("taskChanges", {
      taskId,
      since: options?.since,
      limit: options?.limit,
      path: options?.path,
    });
  },
  connectionInfo() {
    return __prismHost("connectionInfo", {});
  },
  runtimeStatus() {
    return __prismHost("runtimeStatus", {});
  },
  runtimeLogs(options = {}) {
    options = __prismValidateOptions(
      "prism.runtimeLogs",
      options,
      __prismOptionKeys.runtimeLogs
    );
    return __prismHost("runtimeLogs", {
      limit: options?.limit,
      scope: options?.scope,
      worktreeId: options?.worktreeId ?? options?.worktree_id,
      level: options?.level,
      target: options?.target,
      contains: options?.contains,
    });
  },
  runtimeTimeline(options = {}) {
    options = __prismValidateOptions(
      "prism.runtimeTimeline",
      options,
      __prismOptionKeys.runtimeTimeline
    );
    return __prismHost("runtimeTimeline", {
      limit: options?.limit,
      scope: options?.scope,
      worktreeId: options?.worktreeId ?? options?.worktree_id,
      contains: options?.contains,
    });
  },
  validationFeedback(options = {}) {
    options = __prismValidateOptions(
      "prism.validationFeedback",
      options,
      __prismOptionKeys.validationFeedback
    );
    return __prismHost("validationFeedback", {
      limit: options?.limit,
      since: options?.since,
      taskId: options?.taskId ?? options?.task_id,
      verdict: options?.verdict,
      category: options?.category,
      contains: options?.contains,
      correctedManually:
        options?.correctedManually ?? options?.corrected_manually,
    });
  },
  runtime: Object.freeze({
    status() {
      return prism.runtimeStatus();
    },
    logs(options = {}) {
      return prism.runtimeLogs(options);
    },
    timeline(options = {}) {
      return prism.runtimeTimeline(options);
    },
  }),
  connection: Object.freeze({
    info() {
      return prism.connectionInfo();
    },
  }),
  memory: Object.freeze({
    recall(options = {}) {
      options = __prismValidateOptions(
        "prism.memory.recall",
        options,
        __prismOptionKeys.memoryRecall
      );
      return __prismHost("memoryRecall", {
        focus: __prismNormalizeFocus(options.focus),
        text: options.text,
        limit: options.limit,
        kinds: options.kinds,
        since: options.since,
      });
    },
    outcomes(options = {}) {
      options = __prismValidateOptions(
        "prism.memory.outcomes",
        options,
        __prismOptionKeys.memoryOutcomes
      );
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
    events(options = {}) {
      options = __prismValidateOptions(
        "prism.memory.events",
        options,
        __prismOptionKeys.memoryEvents
      );
      return __prismHost("memoryEvents", {
        memoryId: options.memoryId,
        focus: __prismNormalizeFocus(options.focus),
        text: options.text,
        limit: options.limit,
        kinds: options.kinds,
        actions: options.actions,
        scope: options.scope,
        taskId: options.taskId,
        since: options.since,
      });
    },
  }),
  memoryRecall(options = {}) {
    return prism.memory.recall(options);
  },
  memoryOutcomes(options = {}) {
    return prism.memory.outcomes(options);
  },
  memoryEvents(options = {}) {
    return prism.memory.events(options);
  },
  curator: Object.freeze({
    jobs(options = {}) {
      options = __prismValidateOptions(
        "prism.curator.jobs",
        options,
        __prismOptionKeys.curatorJob
      );
      return __prismHost("curatorJobs", options);
    },
    proposals(options = {}) {
      options = __prismValidateOptions(
        "prism.curator.proposals",
        options,
        __prismOptionKeys.curatorProposals
      );
      return __prismHost("curatorProposals", {
        status: options?.status,
        trigger: options?.trigger,
        kind: options?.kind,
        disposition: options?.disposition,
        taskId: options?.taskId ?? options?.task_id,
        limit: options?.limit,
      });
    },
    job(id) {
      if (typeof id !== "string" || id.length === 0) {
        return null;
      }
      return __prismHost("curatorJob", { job_id: id });
    },
  }),
  mcpLog(options = {}) {
    options = __prismValidateOptions("prism.mcpLog", options, __prismOptionKeys.mcpLog);
    return __prismHost("mcpLog", {
      limit: options?.limit,
      since: options?.since,
      scope: options?.scope,
      callType: options?.callType ?? options?.call_type,
      name: options?.name,
      taskId: options?.taskId ?? options?.task_id,
      worktreeId: options?.worktreeId ?? options?.worktree_id,
      repoId: options?.repoId ?? options?.repo_id,
      workspaceRoot: options?.workspaceRoot ?? options?.workspace_root,
      sessionId: options?.sessionId ?? options?.session_id,
      serverInstanceId: options?.serverInstanceId ?? options?.server_instance_id,
      processId: options?.processId ?? options?.process_id,
      success: options?.success,
      minDurationMs: options?.minDurationMs ?? options?.min_duration_ms,
      contains: options?.contains,
    });
  },
  slowMcpCalls(options = {}) {
    options = __prismValidateOptions(
      "prism.slowMcpCalls",
      options,
      __prismOptionKeys.mcpLog
    );
    return __prismHost("slowMcpCalls", {
      limit: options?.limit,
      since: options?.since,
      scope: options?.scope,
      callType: options?.callType ?? options?.call_type,
      name: options?.name,
      taskId: options?.taskId ?? options?.task_id,
      worktreeId: options?.worktreeId ?? options?.worktree_id,
      repoId: options?.repoId ?? options?.repo_id,
      workspaceRoot: options?.workspaceRoot ?? options?.workspace_root,
      sessionId: options?.sessionId ?? options?.session_id,
      serverInstanceId: options?.serverInstanceId ?? options?.server_instance_id,
      processId: options?.processId ?? options?.process_id,
      success: options?.success,
      minDurationMs: options?.minDurationMs ?? options?.min_duration_ms,
      contains: options?.contains,
    });
  },
  mcpTrace(id) {
    if (typeof id !== "string" || id.length === 0) {
      return null;
    }
    return __prismHost("mcpTrace", { id });
  },
  mcpStats(options = {}) {
    options = __prismValidateOptions("prism.mcpStats", options, __prismOptionKeys.mcpLog);
    return __prismHost("mcpStats", {
      since: options?.since,
      scope: options?.scope,
      callType: options?.callType ?? options?.call_type,
      name: options?.name,
      taskId: options?.taskId ?? options?.task_id,
      worktreeId: options?.worktreeId ?? options?.worktree_id,
      repoId: options?.repoId ?? options?.repo_id,
      workspaceRoot: options?.workspaceRoot ?? options?.workspace_root,
      sessionId: options?.sessionId ?? options?.session_id,
      serverInstanceId: options?.serverInstanceId ?? options?.server_instance_id,
      processId: options?.processId ?? options?.process_id,
      success: options?.success,
      minDurationMs: options?.minDurationMs ?? options?.min_duration_ms,
      contains: options?.contains,
    });
  },
  queryLog(options = {}) {
    options = __prismValidateOptions("prism.queryLog", options, __prismOptionKeys.queryLog);
    return __prismHost("queryLog", {
      limit: options?.limit,
      since: options?.since,
      target: options?.target,
      operation: options?.operation,
      taskId: options?.taskId ?? options?.task_id,
      minDurationMs: options?.minDurationMs ?? options?.min_duration_ms,
    });
  },
  slowQueries(options = {}) {
    options = __prismValidateOptions(
      "prism.slowQueries",
      options,
      __prismOptionKeys.queryLog
    );
    return __prismHost("slowQueries", {
      limit: options?.limit,
      since: options?.since,
      target: options?.target,
      operation: options?.operation,
      taskId: options?.taskId ?? options?.task_id,
      minDurationMs: options?.minDurationMs ?? options?.min_duration_ms,
    });
  },
  queryTrace(id) {
    if (typeof id !== "string" || id.length === 0) {
      return null;
    }
    return __prismHost("queryTrace", { id });
  },
  diagnostics() {
    return __prismHost("diagnostics", {});
  },
});

globalThis.prism = new Proxy(__prismBase, {
  get(target, prop, receiver) {
    if (Reflect.has(target, prop)) {
      return Reflect.get(target, prop, receiver);
    }
    if (typeof prop !== "string") {
      return undefined;
    }
    return __prismLoadDynamicViews().get(prop);
  },
  has(target, prop) {
    return (
      Reflect.has(target, prop) ||
      (typeof prop === "string" && __prismLoadDynamicViews().has(prop))
    );
  },
  ownKeys(target) {
    const keys = new Set(Reflect.ownKeys(target));
    for (const name of __prismLoadDynamicViews().keys()) {
      keys.add(name);
    }
    return Array.from(keys);
  },
  getOwnPropertyDescriptor(target, prop) {
    if (Reflect.has(target, prop)) {
      return Object.getOwnPropertyDescriptor(target, prop);
    }
    if (typeof prop !== "string") {
      return undefined;
    }
    const method = __prismLoadDynamicViews().get(prop);
    if (!method) {
      return undefined;
    }
    return {
      configurable: true,
      enumerable: true,
      writable: false,
      value: method,
    };
  },
});

const __prismBaselineGlobals = Object.getOwnPropertyNames(globalThis);
"#;
