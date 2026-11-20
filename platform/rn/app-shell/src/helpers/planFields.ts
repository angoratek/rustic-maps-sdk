/**
 * The plan search fields (§4.1): the two field names, the consumer word a
 * no-match line names, and the markers and formatting the fields render.
 */

/** The plan field the result list currently serves (§4.1's field pair). */
export type PlanSearchField = 'origin' | 'destination';

/** The consumer field word a no-match line names. */
export function planSearchFieldLabel(planSearchField: PlanSearchField): string {
  return planSearchField === 'origin' ? 'origin' : 'destination';
}

/** The A/B markers the two plan fields carry beside their visible labels. */
export const planOriginFieldMarker = 'A';
export const planDestinationFieldMarker = 'B';

/** The decimal places the My location fill gives a device fix (metres). */
export const planFieldDeviceFixDecimalPlaces = 5;

/** The mark a My location chip carries once it filled its own field. */
export const myLocationChipFilledMark = '·';
