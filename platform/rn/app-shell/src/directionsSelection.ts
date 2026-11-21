/**
 * The plan sheet's Directions choice (PRD.md §6 item 36):
 * Car or Walk, with Car as the default, and Fastest or Shortest, with Fastest
 * as the default and shown only for car plans. The serving routing provider
 * reports what it can honour through `routeCapabilities()`; a choice the
 * provider does not support is disabled with its reason instead of sending a
 * request the provider cannot serve.
 *
 * The module stays free of React and native imports so jest pins every
 * decision without a native side. Both choices are remembered as stable
 * lowercase names rather than generated enum numbers, so a future enum
 * addition cannot reinterpret a stored record.
 */

import {
  RouteStrategy,
  TransportationMode,
} from '@angoratek/rustic-maps-sdk/src/generated/rustic_maps.ts';
import type { RouteCapabilities } from '@angoratek/rustic-maps-sdk/MapsSdk';

import { appStrings } from './theme';

/** The transport modes the plan sheet offers (Car first, the default). */
export type DirectionsTransportationModeName = 'automobile' | 'pedestrian';

/** The route strategies the plan sheet offers (Fastest first, the default). */
export type DirectionsRouteStrategyName = 'fastest' | 'shortest';

/** The offered transport modes, in presentation order. */
export const directionsTransportationModeNames: readonly DirectionsTransportationModeName[] =
  ['automobile', 'pedestrian'];

/** The offered route strategies, in presentation order. */
export const directionsRouteStrategyNames: readonly DirectionsRouteStrategyName[] = [
  'fastest',
  'shortest',
];

/** The choice a device that never chose starts from. */
export const defaultDirectionsTransportationModeName: DirectionsTransportationModeName =
  'automobile';
export const defaultDirectionsRouteStrategyName: DirectionsRouteStrategyName =
  'fastest';

/** The plan call's two contract values. */
export interface DirectionsSelection {
  transportationMode: TransportationMode;
  routeStrategy: RouteStrategy;
}

/** Whether one choice can be selected, and the reason when it cannot. */
export interface DirectionsChoiceState {
  isEnabled: boolean;
  /** The verbatim reason, empty while the choice is enabled. */
  reasonText: string;
}

/** Everything the plan sheet's Directions control renders. */
export interface DirectionsControlState {
  /** The values the next plan call sends. */
  selection: DirectionsSelection;
  car: DirectionsChoiceState;
  walk: DirectionsChoiceState;
  fastest: DirectionsChoiceState;
  shortest: DirectionsChoiceState;
  /** The strategy row is shown only for car plans. */
  isStrategyRowVisible: boolean;
  /**
   * Why no capability report is available (no routing provider, a refused
   * read). It disables both mode choices; empty when the report loaded.
   */
  unavailableReasonText: string;
}

/** The contract value a remembered mode name names. */
export function transportationModeForName(
  transportationModeName: DirectionsTransportationModeName,
): TransportationMode {
  return transportationModeName === 'pedestrian'
    ? TransportationMode.Pedestrian
    : TransportationMode.Automobile;
}

/** The contract value a remembered strategy name names. */
export function routeStrategyForName(
  routeStrategyName: DirectionsRouteStrategyName,
): RouteStrategy {
  return routeStrategyName === 'shortest'
    ? RouteStrategy.Shortest
    : RouteStrategy.Fastest;
}

/** The stable stored name of a contract transport mode. */
export function transportationModeNameFor(
  transportationMode: TransportationMode,
): DirectionsTransportationModeName {
  return transportationMode === TransportationMode.Pedestrian
    ? 'pedestrian'
    : 'automobile';
}

/** The stable stored name of a contract route strategy. */
export function routeStrategyNameFor(
  routeStrategy: RouteStrategy,
): DirectionsRouteStrategyName {
  return routeStrategy === RouteStrategy.Shortest ? 'shortest' : 'fastest';
}

/** The plan call's values for the remembered names. */
export function directionsSelectionFromNames(
  transportationModeName: DirectionsTransportationModeName,
  routeStrategyName: DirectionsRouteStrategyName,
): DirectionsSelection {
  return {
    transportationMode: transportationModeForName(transportationModeName),
    routeStrategy: routeStrategyForName(routeStrategyName),
  };
}

/**
 * The control state the plan sheet renders from the remembered choices and
 * the serving provider's report.
 *
 * A mode the provider does not support is disabled with its reason. The
 * selected values a plan sends are the honoured ones: an unsupported mode
 * falls back to the first mode the provider supports (Car first), and a
 * walking plan always sends the fastest strategy because the contract applies
 * the strategy to automobile routing only. The remembered values are left
 * untouched, so returning to a capable provider restores the earlier choice.
 */
export function directionsControlState(
  rememberedSelection: DirectionsSelection,
  routeCapabilities: RouteCapabilities | null,
  unavailableReasonText: string = '',
): DirectionsControlState {
  if (routeCapabilities === null) {
    const unavailableChoice: DirectionsChoiceState = {
      isEnabled: false,
      reasonText: unavailableReasonText,
    };
    return {
      selection: rememberedSelection,
      car: unavailableChoice,
      walk: unavailableChoice,
      fastest: unavailableChoice,
      shortest: unavailableChoice,
      isStrategyRowVisible:
        rememberedSelection.transportationMode === TransportationMode.Automobile,
      unavailableReasonText,
    };
  }
  const supportedModes = routeCapabilities.supportedTransportationModes;
  const isCarSupported = supportedModes.includes(TransportationMode.Automobile);
  const isWalkSupported = supportedModes.includes(TransportationMode.Pedestrian);
  const isShortestSupported =
    isCarSupported && routeCapabilities.supportsShortestRouteStrategy;

  let effectiveTransportationMode = rememberedSelection.transportationMode;
  if (!supportedModes.includes(effectiveTransportationMode)) {
    effectiveTransportationMode = isCarSupported
      ? TransportationMode.Automobile
      : isWalkSupported
        ? TransportationMode.Pedestrian
        : rememberedSelection.transportationMode;
  }
  const isAutomobile =
    effectiveTransportationMode === TransportationMode.Automobile;
  const effectiveRouteStrategy =
    isAutomobile &&
    rememberedSelection.routeStrategy === RouteStrategy.Shortest &&
    isShortestSupported
      ? RouteStrategy.Shortest
      : RouteStrategy.Fastest;

  return {
    selection: {
      transportationMode: effectiveTransportationMode,
      routeStrategy: effectiveRouteStrategy,
    },
    car: {
      isEnabled: isCarSupported,
      reasonText: isCarSupported ? '' : appStrings.directionsCarUnsupportedText,
    },
    walk: {
      isEnabled: isWalkSupported,
      reasonText: isWalkSupported ? '' : appStrings.directionsWalkUnsupportedText,
    },
    fastest: { isEnabled: true, reasonText: '' },
    shortest: {
      isEnabled: isShortestSupported,
      reasonText: isShortestSupported
        ? ''
        : appStrings.directionsShortestUnsupportedText,
    },
    isStrategyRowVisible: isAutomobile,
    unavailableReasonText: '',
  };
}
