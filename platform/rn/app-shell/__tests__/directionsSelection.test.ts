/**
 * The plan sheet's Directions control (docs/design/DirectionsModesAndStrategy.md):
 * Car and Fastest are the defaults, the strategy row is shown only for car
 * plans, and a choice the serving routing provider cannot honour is disabled
 * with its reason while the plan call sends an honoured pair.
 */

import {
  RouteStrategy,
  TransportationMode,
} from 'maps-sdk-rn/src/generated/rustic_maps.ts';

import {
  defaultDirectionsRouteStrategyName,
  defaultDirectionsTransportationModeName,
  directionsControlState,
  directionsSelectionFromNames,
  routeStrategyForName,
  routeStrategyNameFor,
  transportationModeForName,
  transportationModeNameFor,
} from '../src/directionsSelection';
import { appStrings } from '../src/theme';

/** Every shipped routing provider's answer: all modes, shortest supported. */
const allSupportedCapabilities = {
  supportedTransportationModes: [
    TransportationMode.Automobile,
    TransportationMode.Bicycle,
    TransportationMode.Pedestrian,
  ],
  supportsShortestRouteStrategy: true,
};

test('Car and Fastest are the defaults, with the strategy row shown', () => {
  expect(defaultDirectionsTransportationModeName).toBe('automobile');
  expect(defaultDirectionsRouteStrategyName).toBe('fastest');

  const control = directionsControlState(
    directionsSelectionFromNames('automobile', 'fastest'),
    allSupportedCapabilities,
  );
  expect(control.selection).toEqual({
    transportationMode: TransportationMode.Automobile,
    routeStrategy: RouteStrategy.Fastest,
  });
  expect(control.car.isEnabled).toBe(true);
  expect(control.walk.isEnabled).toBe(true);
  expect(control.shortest.isEnabled).toBe(true);
  expect(control.isStrategyRowVisible).toBe(true);
});

test('a provider without shortest support disables that choice with its reason', () => {
  const control = directionsControlState(
    directionsSelectionFromNames('automobile', 'shortest'),
    {
      supportedTransportationModes: [
        TransportationMode.Automobile,
        TransportationMode.Pedestrian,
      ],
      supportsShortestRouteStrategy: false,
    },
  );
  expect(control.shortest.isEnabled).toBe(false);
  expect(control.shortest.reasonText).toBe(
    appStrings.directionsShortestUnsupportedText,
  );
  // The plan sends the honoured strategy even though the remembered one is
  // shortest; returning to a capable provider restores it.
  expect(control.selection.routeStrategy).toBe(RouteStrategy.Fastest);
});

test('a provider without walking support disables Walk with its reason', () => {
  const control = directionsControlState(
    directionsSelectionFromNames('automobile', 'fastest'),
    {
      supportedTransportationModes: [TransportationMode.Automobile],
      supportsShortestRouteStrategy: true,
    },
  );
  expect(control.car.isEnabled).toBe(true);
  expect(control.walk.isEnabled).toBe(false);
  expect(control.walk.reasonText).toBe(
    appStrings.directionsWalkUnsupportedText,
  );
});

test('an unsupported remembered mode falls back to an honoured mode', () => {
  const control = directionsControlState(
    directionsSelectionFromNames('automobile', 'shortest'),
    {
      supportedTransportationModes: [TransportationMode.Pedestrian],
      supportsShortestRouteStrategy: false,
    },
  );
  expect(control.car.isEnabled).toBe(false);
  expect(control.car.reasonText).toBe(appStrings.directionsCarUnsupportedText);
  expect(control.selection.transportationMode).toBe(
    TransportationMode.Pedestrian,
  );
  // A walking plan sends fastest and hides the strategy row.
  expect(control.selection.routeStrategy).toBe(RouteStrategy.Fastest);
  expect(control.isStrategyRowVisible).toBe(false);
});

test('a walking plan hides the strategy row but keeps the car choice', () => {
  const control = directionsControlState(
    directionsSelectionFromNames('pedestrian', 'shortest'),
    allSupportedCapabilities,
  );
  expect(control.isStrategyRowVisible).toBe(false);
  expect(control.selection.routeStrategy).toBe(RouteStrategy.Fastest);
  expect(control.shortest.isEnabled).toBe(true);
});

test('an unavailable capability report disables both modes with the reason', () => {
  const control = directionsControlState(
    directionsSelectionFromNames('automobile', 'fastest'),
    null,
    'ProviderNotConfigured: RoutingProvider',
  );
  expect(control.car.isEnabled).toBe(false);
  expect(control.walk.isEnabled).toBe(false);
  expect(control.car.reasonText).toBe(
    'ProviderNotConfigured: RoutingProvider',
  );
  expect(control.unavailableReasonText).toBe(
    'ProviderNotConfigured: RoutingProvider',
  );
});

test('the stored names round-trip to the contract values and back', () => {
  expect(transportationModeForName('automobile')).toBe(
    TransportationMode.Automobile,
  );
  expect(transportationModeForName('pedestrian')).toBe(
    TransportationMode.Pedestrian,
  );
  expect(routeStrategyForName('fastest')).toBe(RouteStrategy.Fastest);
  expect(routeStrategyForName('shortest')).toBe(RouteStrategy.Shortest);
  expect(transportationModeNameFor(TransportationMode.Automobile)).toBe(
    'automobile',
  );
  expect(transportationModeNameFor(TransportationMode.Pedestrian)).toBe(
    'pedestrian',
  );
  expect(routeStrategyNameFor(RouteStrategy.Fastest)).toBe('fastest');
  expect(routeStrategyNameFor(RouteStrategy.Shortest)).toBe('shortest');
});
