/**
 * The Location group's device-driven navigation copy and its fallback fix
 * accuracy, shared by the plan sheet and the navigation drawer.
 */

/**
 * The Location group's second control (the shared copy of every reference
 * application): device-driven navigation follows the planned route from the
 * same location source's fixes, so the banner, the ETA line and the follow
 * camera read real fixes instead of the recorded trace.
 */
export const deviceNavigationStartText = 'Navigate from my location';
export const deviceNavigationStopText = 'Stop device navigation';
export const deviceNavigationPlanFirstText =
  'Plan a route first; device navigation follows the route you planned.';
export const deviceNavigationWaitingText = 'Waiting for your position to start guidance.';
export const deviceNavigationGuidanceUnavailableText =
  'Guidance could not start on the planned route.';
/**
 * The accuracy a device fix is delivered at when its source carries none: the
 * navigation reducer needs a graded fix to advance steps and judge
 * deviations, so the accepted fix is reported at the recorded trace's own
 * accuracy value.
 */
export const deviceNavigationFallbackAccuracyMeters = 8.0;
