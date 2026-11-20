/**
 * The scene selections in artifact order, for the remembered preferences
 * record.
 */

import { overlayScenes } from '../referenceScenes';

/** The selected scene keys in artifact order (the remembered record's order). */
export function rememberedSceneKeys(
  sceneSelections: Record<string, boolean>,
): string[] {
  return overlayScenes
    .filter(({ sceneKey }) => sceneSelections[sceneKey])
    .map(({ sceneKey }) => sceneKey);
}
