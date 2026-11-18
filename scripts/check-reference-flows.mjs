// Reference-flow gate: the ordered flows every reference application must run
// (docs/design/ReferenceApplicationBehavior.md).
//
// The gate reads `platform/reference-flows.json`, the shared copy artifact
// (`platform/reference-tokens.json`), the provider registry
// (`platform/providers.json`) and the platform sources the flow file names. It
// fails on:
//   1. a copy ID that is not in `ui_labels`;
//   2. a visible or condition ID that is not in the flow file's component
//      vocabulary, or a state ID that is not in its state vocabulary;
//   3. a flow without an explicit screenshot decision, a reference flow whose
//      published frame set is not the four documented frames in order, or a
//      screenshot step that does not show the reference scenario at its
//      documented moment;
//   4. a platform hook that is missing without a reason, does not use that
//      platform's hook kind, or does not occur in the declared source while it
//      is not marked pending;
//   5. a keyless routing descriptor without a public demo address and terms
//      note, a flow that selects a provider the registry cannot serve, or a
//      provider URL literal in a reference-application source that is not a
//      registry value (the registry is the URL's only home);
//   6. a missing `reference-flows.json` build input, or a pending input
//      without a reason.
//
// Run directly (no server or browser involved):
//   node scripts/check-reference-flows.mjs

import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REFERENCE_FLOWS_PATH = 'platform/reference-flows.json';
const REFERENCE_TOKENS_PATH = 'platform/reference-tokens.json';
const PROVIDER_REGISTRY_PATH = 'platform/providers.json';
const FLOW_REDUCER_SOURCE_PATH = 'crates/map-refapp-core/src/flow.rs';

const FLOW_PLATFORM_KEYS = ['android', 'ios', 'react_native', 'web'];
/** The four frames the reference screenshots page publishes, in page order. */
const SCREENSHOT_FRAMES = [
  'plan_sheet',
  'provider_section',
  'navigation',
  'vector_base',
];
const OUTCOME_KINDS = new Set(['state', 'visible', 'condition', 'copy']);
const PLAN_FIELDS = new Set(['origin', 'destination']);
const DIRECTORY_NAMES_TO_SKIP = new Set([
  'androidTest',
  'build',
  'DerivedData',
  'node_modules',
  'tests',
  '__tests__',
]);
const SOURCE_FILE_EXTENSIONS = new Set(['.html', '.kt', '.mjs', '.swift', '.ts', '.tsx']);

/** The URLs a reference-application source may spell, beyond the registry. */
const HTTP_URL_PATTERN = /https?:\/\/[^\s"'<>)\]]+/g;

/** Every HTTP(S) URL in one text, without trailing prose punctuation. */
function httpUrlsIn(text) {
  return [...text.matchAll(HTTP_URL_PATTERN)]
    .map((match) => match[0].replace(/[.,;:]+$/, ''));
}

/** Collects every HTTP(S) URL a registry value declares, recursively. */
function collectRegistryUrls(value, target) {
  if (typeof value === 'string') {
    for (const url of httpUrlsIn(value)) target.add(url);
    return;
  }
  if (Array.isArray(value)) {
    for (const entry of value) collectRegistryUrls(entry, target);
    return;
  }
  if (value !== null && typeof value === 'object') {
    for (const entry of Object.values(value)) collectRegistryUrls(entry, target);
  }
}

const REPOSITORY_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
);

/** Reads one repository-relative file as UTF-8. */
function read(relative_path, repository_root = REPOSITORY_ROOT) {
  return readFileSync(path.join(repository_root, relative_path), 'utf8');
}

/** Reads one repository-relative JSON file. */
function readJson(relative_path, repository_root = REPOSITORY_ROOT) {
  return JSON.parse(read(relative_path, repository_root));
}

/** Whether a value is a non-empty string. */
function isNonEmptyString(value) {
  return typeof value === 'string' && value.length > 0;
}

/** Collects the text of every source file under one path. */
function collectSourceText(relative_path, repository_root) {
  const absolute_path = path.join(repository_root, relative_path);
  if (!existsSync(absolute_path)) return '';
  const status = statSync(absolute_path);
  if (status.isFile()) return readFileSync(absolute_path, 'utf8');
  const chunks = [];
  for (const entry of readdirSync(absolute_path)) {
    if (DIRECTORY_NAMES_TO_SKIP.has(entry)) continue;
    const entry_path = path.join(absolute_path, entry);
    const entry_status = statSync(entry_path);
    if (entry_status.isDirectory()) {
      chunks.push(collectSourceText(path.join(relative_path, entry), repository_root));
    } else if (SOURCE_FILE_EXTENSIONS.has(path.extname(entry))) {
      chunks.push(readFileSync(entry_path, 'utf8'));
    }
  }
  return chunks.join('\n');
}

/** Whether a hook's literal identifier occurs in the collected source text. */
function hookOccursInSource(source_text, hook) {
  const identifier = hook.kind === 'css_selector' && hook.value.startsWith('#')
    ? hook.value.slice(1)
    : hook.value;
  return source_text.includes(identifier);
}

/**
 * Runs every reference-flow check against a repository root.
 *
 * Returns a small report so the caller can print the pending-hook adoption
 * state; throws with every violation when the schema disagrees with the
 * artifacts or the sources.
 *
 * @param {string} [repository_root]
 * @returns {{flows: number, steps: number, pendingHooks: string[]}}
 */
export function run_reference_flow_checks(repository_root = REPOSITORY_ROOT) {
  const failures = [];
  const pending_hooks = [];
  const flow_artifact = readJson(REFERENCE_FLOWS_PATH, repository_root);
  const tokens_artifact = readJson(REFERENCE_TOKENS_PATH, repository_root);
  const provider_registry = readJson(PROVIDER_REGISTRY_PATH, repository_root);

  if (flow_artifact.schema_version !== 1) {
    failures.push(`${REFERENCE_FLOWS_PATH}: schema_version must be 1`);
  }
  if (flow_artifact.artifact?.name !== 'reference-flows') {
    failures.push(`${REFERENCE_FLOWS_PATH}: artifact.name must be "reference-flows"`);
  }
  if (!isNonEmptyString(flow_artifact.artifact?.purpose)) {
    failures.push(`${REFERENCE_FLOWS_PATH}: artifact.purpose must be documented`);
  }

  const state_ids = new Set(
    (flow_artifact.states ?? []).map((state) => state.id).filter(isNonEmptyString),
  );
  const component_ids = new Set(
    (flow_artifact.component_vocabulary ?? [])
      .map((component) => component.id)
      .filter(isNonEmptyString),
  );
  const action_kinds = new Set(flow_artifact.action_kinds ?? []);
  const ui_label_ids = new Set(
    (tokens_artifact.ui_labels ?? []).map((label) => label.id).filter(isNonEmptyString),
  );
  const provider_descriptors = new Map(
    (provider_registry.providers ?? []).map((descriptor) => [
      descriptor.provider_key,
      descriptor,
    ]),
  );

  if (state_ids.size === 0) {
    failures.push(`${REFERENCE_FLOWS_PATH}: the states vocabulary is empty`);
  }
  if (component_ids.size === 0) {
    failures.push(`${REFERENCE_FLOWS_PATH}: the component vocabulary is empty`);
  }
  if (action_kinds.size === 0) {
    failures.push(`${REFERENCE_FLOWS_PATH}: the action_kinds vocabulary is empty`);
  }
  if (ui_label_ids.size === 0) {
    failures.push(`${REFERENCE_TOKENS_PATH}: the ui_labels section is empty`);
  }

  checkRegistryDemoEndpoints(provider_descriptors, failures);
  checkPlatformDeclarations(flow_artifact.platforms, failures, repository_root);
  const platform_source_text = new Map(
    FLOW_PLATFORM_KEYS.map((platform) => [
      platform,
      (flow_artifact.platforms?.[platform]?.source_paths ?? [])
        .map((source_path) => collectSourceText(source_path, repository_root))
        .join('\n'),
    ]),
  );
  checkSourceUrls(platform_source_text, provider_registry, failures);
  checkReducerCopyIds(ui_label_ids, failures, repository_root);

  const flows = Array.isArray(flow_artifact.flows) ? flow_artifact.flows : [];
  if (flows.length === 0) {
    failures.push(`${REFERENCE_FLOWS_PATH}: the flows list is empty`);
  }
  const flow_ids = new Set();
  let total_steps = 0;
  let reference_flow_count = 0;

  for (const flow of flows) {
    const flow_path = `flow "${flow.flow_id}"`;
    if (!isNonEmptyString(flow.flow_id)) {
      failures.push(`${REFERENCE_FLOWS_PATH}: a flow has no flow_id`);
      continue;
    }
    if (flow_ids.has(flow.flow_id)) {
      failures.push(`${REFERENCE_FLOWS_PATH}: flow_id "${flow.flow_id}" is declared twice`);
    }
    flow_ids.add(flow.flow_id);
    if (!isNonEmptyString(flow.label)) {
      failures.push(`${flow_path}: label must be documented`);
    }
    if (!isNonEmptyString(flow.scenario)) {
      failures.push(`${flow_path}: scenario must be documented`);
    }
    if (flow.scenario === 'reference') reference_flow_count += 1;

    if (!Object.hasOwn(flow, 'screenshot_step_id')) {
      failures.push(`${flow_path}: no screenshot decision (screenshot_step_id is missing)`);
    }
    const steps = Array.isArray(flow.steps) ? flow.steps : [];
    if (steps.length === 0) {
      failures.push(`${flow_path}: has no steps`);
    }
    const step_ids = new Set();
    for (const step of steps) {
      total_steps += 1;
      const step_path = `${flow_path} step "${step.step_id}"`;
      if (!isNonEmptyString(step.step_id)) {
        failures.push(`${flow_path}: a step has no step_id`);
        continue;
      }
      if (step_ids.has(step.step_id)) {
        failures.push(`${flow_path}: step_id "${step.step_id}" is declared twice`);
      }
      step_ids.add(step.step_id);
      checkAction(step, action_kinds, provider_descriptors, step_path, failures);
      checkOutcomes(step, state_ids, component_ids, ui_label_ids, step_path, failures);
      checkHooks(
        step,
        flow_artifact.platforms,
        platform_source_text,
        step_path,
        failures,
        pending_hooks,
      );
    }

    if (flow.scenario === 'reference') {
      if (!isNonEmptyString(flow.screenshot_step_id)) {
        failures.push(`${flow_path}: the reference scenario must name its screenshot step`);
      } else if (!step_ids.has(flow.screenshot_step_id)) {
        failures.push(
          `${flow_path}: screenshot_step_id "${flow.screenshot_step_id}" is not a step`,
        );
      } else {
        checkScreenshotStep(flow, failures);
        checkScreenshotFrames(flow, failures);
      }
    } else if (flow.screenshot_step_id !== null) {
      failures.push(`${flow_path}: a non-reference flow must set screenshot_step_id to null`);
    }
  }

  if (reference_flow_count !== 1) {
    failures.push(
      `${REFERENCE_FLOWS_PATH}: exactly one flow must carry scenario "reference" `
        + `(found ${reference_flow_count})`,
    );
  }

  if (failures.length > 0) {
    throw new Error(
      `The reference-flow contract has ${failures.length} violation(s):\n- `
        + failures.join('\n- '),
    );
  }
  return { flows: flows.length, steps: total_steps, pendingHooks: pending_hooks };
}

/**
 * A reference-application source may not spell a service URL the registry does
 * not declare. Dynamic values read from the registry are not literals and do
 * not appear here; a hard-coded provider address is exactly the leak this
 * check exists to stop.
 */
function checkSourceUrls(platform_source_text, provider_registry, failures) {
  const registry_urls = new Set();
  collectRegistryUrls(provider_registry, registry_urls);
  for (const [platform, source_text] of platform_source_text) {
    for (const url of httpUrlsIn(source_text)) {
      if (registry_urls.has(url)) continue;
      failures.push(
        `platform "${platform}": the source spells the URL "${url}", `
          + `which ${PROVIDER_REGISTRY_PATH} does not declare`,
      );
    }
  }
}

/**
 * The reducer's copy-ID literals must exist in `ui_labels` too, so the Rust
 * decision layer and the shared copy artifact cannot drift.
 */
function checkReducerCopyIds(ui_label_ids, failures, repository_root) {
  const source_text = read(FLOW_REDUCER_SOURCE_PATH, repository_root);
  const dotted_literal_pattern = /"([a-z_]+\.[a-z_]+)"/g;
  const seen = new Set();
  for (const match of source_text.matchAll(dotted_literal_pattern)) {
    const copy_id = match[1];
    if (seen.has(copy_id)) continue;
    seen.add(copy_id);
    if (!ui_label_ids.has(copy_id)) {
      failures.push(
        `${FLOW_REDUCER_SOURCE_PATH}: copy ID "${copy_id}" is not in ui_labels`,
      );
    }
  }
}

/** Keyless routing descriptors must declare a public demo address and terms. */
function checkRegistryDemoEndpoints(provider_descriptors, failures) {  for (const [provider_key, descriptor] of provider_descriptors) {
    const serves_routing = (descriptor.served_contracts ?? []).includes('routing');
    if (!serves_routing || descriptor.credential_label !== '') continue;
    if (!isNonEmptyString(descriptor.demo_base_url)
      || !descriptor.demo_base_url.startsWith('https://')) {
      failures.push(
        `${PROVIDER_REGISTRY_PATH}: keyless routing provider "${provider_key}" `
          + 'must declare an https demo_base_url',
      );
    }
    if (!isNonEmptyString(descriptor.demo_base_url_note)) {
      failures.push(
        `${PROVIDER_REGISTRY_PATH}: keyless routing provider "${provider_key}" `
          + 'must declare a demo_base_url_note',
      );
    }
    if (!isNonEmptyString(descriptor.terms_note)) {
      failures.push(
        `${PROVIDER_REGISTRY_PATH}: provider "${provider_key}" must declare a terms_note`,
      );
    }
  }
}

/** Every platform must declare a hook kind, sources and a build input. */
function checkPlatformDeclarations(platforms, failures, repository_root) {
  if (platforms === null || typeof platforms !== 'object') {
    failures.push(`${REFERENCE_FLOWS_PATH}: the platforms section is missing`);
    return;
  }
  const declared_keys = Object.keys(platforms).sort();
  if (declared_keys.join(',') !== [...FLOW_PLATFORM_KEYS].sort().join(',')) {
    failures.push(
      `${REFERENCE_FLOWS_PATH}: platforms must declare exactly `
        + `${FLOW_PLATFORM_KEYS.join(', ')}`,
    );
  }
  for (const platform of FLOW_PLATFORM_KEYS) {
    const declaration = platforms[platform];
    if (declaration === undefined || typeof declaration !== 'object') {
      failures.push(`${REFERENCE_FLOWS_PATH}: platform "${platform}" is not declared`);
      continue;
    }
    if (!isNonEmptyString(declaration.hook_kind)) {
      failures.push(`platform "${platform}": hook_kind must be declared`);
    }
    if (!Array.isArray(declaration.source_paths) || declaration.source_paths.length === 0) {
      failures.push(`platform "${platform}": source_paths must be declared`);
    }
    const build_input = declaration.build_input;
    if (build_input === undefined || !isNonEmptyString(build_input.path)) {
      failures.push(`platform "${platform}": build_input.path must be declared`);
      continue;
    }
    if (build_input.pending === true) {
      if (!isNonEmptyString(build_input.reason)) {
        failures.push(
          `platform "${platform}": a pending build input needs a reason`,
        );
      }
      continue;
    }
    if (!existsSync(path.join(repository_root, build_input.path))) {
      failures.push(
        `platform "${platform}": build input "${build_input.path}" does not exist`,
      );
      continue;
    }
    if (!read(build_input.path, repository_root).includes('reference-flows.json')) {
      failures.push(
        `platform "${platform}": build input "${build_input.path}" does not carry `
          + 'reference-flows.json',
      );
    }
  }
}

/** One step's action must use the declared vocabulary and fields. */
function checkAction(step, action_kinds, provider_descriptors, step_path, failures) {
  const action = step.action;
  if (action === null || typeof action !== 'object') {
    failures.push(`${step_path}: action is missing`);
    return;
  }
  if (!action_kinds.has(action.kind)) {
    failures.push(`${step_path}: action kind "${action.kind}" is not declared`);
  }
  if (['open_search', 'type_and_search', 'assign_result', 'my_location'].includes(action.kind)
    && !PLAN_FIELDS.has(action.field)) {
    failures.push(`${step_path}: action "${action.kind}" needs field origin or destination`);
  }
  if (['tap', 'verify', 'wait_for_state'].includes(action.kind)
    && !isNonEmptyString(action.target)) {
    failures.push(`${step_path}: action "${action.kind}" needs a target`);
  }
  if (action.kind === 'type_and_search' && !isNonEmptyString(action.value)) {
    failures.push(`${step_path}: a type_and_search action needs a value`);
  }
  if (action.kind === 'assign_result' && !isNonEmptyString(action.value)) {
    failures.push(`${step_path}: an assign_result action needs a value`);
  }
  if (action.kind === 'select_provider') {
    const descriptor = provider_descriptors.get(action.provider_key);
    if (descriptor === undefined) {
      failures.push(
        `${step_path}: provider_key "${action.provider_key}" is not in the registry`,
      );
    } else if (!(descriptor.served_contracts ?? []).includes('routing')) {
      failures.push(
        `${step_path}: provider "${action.provider_key}" does not serve routing`,
      );
    } else if (!isNonEmptyString(descriptor.demo_base_url)) {
      failures.push(
        `${step_path}: provider "${action.provider_key}" has no demo_base_url`,
      );
    }
  }
}

/** Every outcome must name a declared state, component or copy ID. */
function checkOutcomes(step, state_ids, component_ids, ui_label_ids, step_path, failures) {
  const outcomes = Array.isArray(step.outcomes) ? step.outcomes : [];
  if (outcomes.length === 0) {
    failures.push(`${step_path}: has no outcomes`);
  }
  for (const outcome of outcomes) {
    if (!OUTCOME_KINDS.has(outcome.kind)) {
      failures.push(`${step_path}: outcome kind "${outcome.kind}" is not declared`);
      continue;
    }
    if (!isNonEmptyString(outcome.id)) {
      failures.push(`${step_path}: an outcome has no id`);
      continue;
    }
    if (outcome.kind === 'state' && !state_ids.has(outcome.id)) {
      failures.push(`${step_path}: state "${outcome.id}" is not in the states vocabulary`);
    }
    if (outcome.kind === 'copy' && !ui_label_ids.has(outcome.id)) {
      failures.push(`${step_path}: copy ID "${outcome.id}" is not in ui_labels`);
    }
    if ((outcome.kind === 'visible' || outcome.kind === 'condition')
      && !component_ids.has(outcome.id)) {
      failures.push(
        `${step_path}: component "${outcome.id}" is not in the component vocabulary`,
      );
    }
  }
}

/** Every platform hook must be declared, typed and present in its source. */
function checkHooks(
  step,
  platforms,
  platform_source_text,
  step_path,
  failures,
  pending_hooks,
) {
  const hooks = step.hooks;
  if (hooks === null || typeof hooks !== 'object') {
    failures.push(`${step_path}: hooks are missing`);
    return;
  }
  for (const platform of FLOW_PLATFORM_KEYS) {
    const hook = hooks[platform];
    const declaration = platforms?.[platform];
    if (hook === undefined) {
      failures.push(
        `${step_path}: no ${platform} hook and no not_available reason`,
      );
      continue;
    }
    if (hook.kind === 'not_available') {
      if (!isNonEmptyString(hook.reason)) {
        failures.push(`${step_path}: ${platform} not_available hook needs a reason`);
      }
      continue;
    }
    if (hook.kind !== declaration?.hook_kind) {
      failures.push(
        `${step_path}: ${platform} hook kind "${hook.kind}" `
          + `must be "${declaration?.hook_kind}"`,
      );
      continue;
    }
    if (!isNonEmptyString(hook.value)) {
      failures.push(`${step_path}: ${platform} hook has no value`);
      continue;
    }
    if (hook.pending === true) {
      if (!isNonEmptyString(hook.reason)) {
        failures.push(`${step_path}: ${platform} pending hook needs a reason`);
        continue;
      }
      pending_hooks.push(`${step.step_id}/${platform}: ${hook.value}`);
      continue;
    }
    const source_text = platform_source_text.get(platform) ?? '';
    if (!hookOccursInSource(source_text, hook)) {
      failures.push(
        `${step_path}: ${platform} hook "${hook.value}" does not occur in `
          + `"${declaration.source_paths.join(', ')}" and is not marked pending`,
      );
    }
  }
  for (const platform of Object.keys(hooks)) {
    if (!FLOW_PLATFORM_KEYS.includes(platform)) {
      failures.push(`${step_path}: hook for unknown platform "${platform}"`);
    }
  }
}

/** The reference flow must publish the four documented frames in order. */
function checkScreenshotFrames(flow, failures) {
  const frames = flow.screenshot_frames;
  const matches = Array.isArray(frames)
    && frames.length === SCREENSHOT_FRAMES.length
    && SCREENSHOT_FRAMES.every((frame, index) => frames[index] === frame);
  if (!matches) {
    failures.push(
      `flow "${flow.flow_id}" must publish the four documented frames in order: `
        + SCREENSHOT_FRAMES.join(', '),
    );
  }
}

/** The reference screenshot must show the documented navigation scenario. */
function checkScreenshotStep(flow, failures) {
  const screenshot_step = flow.steps.find((step) => step.step_id === flow.screenshot_step_id);
  const visible_ids = new Set(
    (screenshot_step?.outcomes ?? [])
      .filter((outcome) => outcome.kind === 'visible')
      .map((outcome) => outcome.id),
  );
  for (const required_component of ['turn_banner', 'route_polyline', 'eta_line', 'speed_controls']) {
    if (!visible_ids.has(required_component)) {
      failures.push(
        `flow "${flow.flow_id}" screenshot step "${flow.screenshot_step_id}" `
          + `does not show "${required_component}"`,
      );
    }
  }
  const condition_ids = new Set(
    (screenshot_step?.outcomes ?? [])
      .filter((outcome) => outcome.kind === 'condition')
      .map((outcome) => outcome.id),
  );
  for (const required_condition of ['first_instruction_shown', 'navigation_speed_one']) {
    if (!condition_ids.has(required_condition)) {
      failures.push(
        `flow "${flow.flow_id}" screenshot step "${flow.screenshot_step_id}" `
          + `does not hold "${required_condition}"`,
      );
    }
  }
  const copy_ids = new Set(
    (screenshot_step?.outcomes ?? [])
      .filter((outcome) => outcome.kind === 'copy')
      .map((outcome) => outcome.id),
  );
  if (!copy_ids.has('navigation.speed_label')) {
    failures.push(
      `flow "${flow.flow_id}" screenshot step "${flow.screenshot_step_id}" `
        + 'does not render navigation.speed_label',
    );
  }
}

/** The `node --test` suite name of the reference-flow assertions. */
const REFERENCE_FLOW_TEST_NAME =
  'the four reference flows agree with the shared copy, registry and platform sources';

const RUNNING_AS_ENTRY = process.argv[1] !== undefined
  && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (RUNNING_AS_ENTRY) {
  try {
    const report = run_reference_flow_checks();
    console.log(
      `PASS: ${report.flows} flows and ${report.steps} steps agree with the shared copy, `
        + 'the provider registry and the declared platform sources.',
    );
    if (report.pendingHooks.length > 0) {
      console.log(
        `  ${report.pendingHooks.length} hook(s) are pending platform adoption: `
          + report.pendingHooks.join(', '),
      );
    }
  } catch (error) {
    console.error(`FAIL: ${error.message}`);
    process.exitCode = 1;
  }
}

export { REFERENCE_FLOW_TEST_NAME };
