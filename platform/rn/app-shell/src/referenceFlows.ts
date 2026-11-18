/**
 * The shared reference-application flow schema
 * (docs/ReferenceAppUX.md): the ordered flows every
 * target runs, with each step's action, visible outcomes, copy IDs and
 * per-platform automation hook. The RN shell consumes
 * `platform/reference-flows.json` by name through a Metro `require`, exactly
 * the file the Gradle copy, the SwiftPM resource and the dev server carry, so
 * the flow vocabulary cannot drift.
 *
 * The shell does not drive itself from the schema: the flow runner is the
 * platform's own user-path methods, and `scripts/check-reference-flows.mjs`
 * proves the schema's test IDs exist in `App.tsx` and its sources. The module
 * exists so the artifact is a declared React Native build input and a future
 * launch-automation runner can read the same file.
 */

import referenceFlowsArtifact from '../../../reference-flows.json';

/** One platform automation hook of a flow step. */
export interface ReferenceFlowHook {
  /** The hook mechanism; React Native uses `test_id`. */
  kind: string;
  /** The stable identifier, or the reason a platform has no surface. */
  value?: string;
  /** Whether a platform slice has not added the identifier yet. */
  pending?: boolean;
  /** Why the identifier is pending or unavailable. */
  reason?: string;
}

/** One visible outcome of a flow step. */
export interface ReferenceFlowOutcome {
  /** `state`, `visible`, `condition` or `copy`. */
  kind: string;
  /** The state, component or copy ID the outcome names. */
  id: string;
}

/** One ordered step of a reference flow. */
export interface ReferenceFlowStep {
  /** The step's stable identifier. */
  step_id: string;
  /** The action a tester performs. */
  action: {
    kind: string;
    field?: 'origin' | 'destination';
    value?: string;
    target?: string;
    provider_key?: string;
  };
  /** What the tester should see. */
  outcomes: ReferenceFlowOutcome[];
  /** The per-platform automation hooks. */
  hooks: Record<string, ReferenceFlowHook>;
}

/** One reference flow. */
export interface ReferenceFlow {
  flow_id: string;
  label: string;
  scenario: string;
  /** The step the screenshot scenario captures, or null. */
  screenshot_step_id: string | null;
  preconditions: string[];
  steps: ReferenceFlowStep[];
}

/** The shared flow artifact. */
export interface ReferenceFlows {
  schema_version: number;
  artifact: { name: string; purpose: string };
  platforms: Record<
    string,
    {
      display_name: string;
      hook_kind: string;
      source_paths: string[];
      build_input: {
        path: string;
        mechanism: string;
        pending?: boolean;
        reason?: string;
      };
    }
  >;
  flows: ReferenceFlow[];
}

export const referenceFlows: ReferenceFlows =
  referenceFlowsArtifact as ReferenceFlows;

/** The flow with the given ID, or null when the artifact declares none. */
export function referenceFlowNamed(flowId: string): ReferenceFlow | null {
  return referenceFlows.flows.find(flow => flow.flow_id === flowId) ?? null;
}

/** One platform's hook for a step, or null when the hook is unavailable. */
export function referenceFlowHook(
  step: ReferenceFlowStep,
  platformKey: string,
): ReferenceFlowHook | null {
  return step.hooks[platformKey] ?? null;
}
