/**
 * The developer console's render contract (M58): the floating control and its
 * Warning/Error dot, the level filter, the newest-first rows from an injected
 * event window, the detail sheet with the typed trace and cache rows, the
 * export through the share sheet, and the honest empty state.
 */

import React from 'react';
import ReactTestRenderer from 'react-test-renderer';
import { Share, Text } from 'react-native';

import {
  DeveloperConsole,
  developerConsoleExportSharedText,
} from '../src/DeveloperConsole';
import type { DiagnosticEvent } from '../src/diagnosticsStream';
import type {
  TileCacheStatistics,
  TrafficFlowCacheStatistics,
} from 'maps-sdk-rn/MapsSdk';
import type { FrameStatisticsEvent } from '../src/RefappNative';
import { buildScreenStyles } from '../src/screenStyles';
import { themeColorsByThemeName } from '../src/theme';

const styles = buildScreenStyles(themeColorsByThemeName.dark);

/** The registry display name the console composes rows with. */
const displayNameForProvider = (providerKey: string): string =>
  providerKey === 'mapquest' ? 'MapQuest' : providerKey;

/** One provider trace, already redacted at emission. */
const trace = {
  providerKey: 'mapquest',
  providerContract: 'RoutingProvider',
  httpMethod: 'GET',
  requestPath: '/directions/v2/route',
  statusClass: '2xx',
  outcome: 'success',
  durationMilliseconds: 123,
  responseBytes: 4096,
  responseExcerpt: '{"route":{"distance":1.2}}',
};

/** The injected fake-listener window, oldest first. */
const events: DiagnosticEvent[] = [
  {
    timestampMillisecondsSinceEpoch: 1_700_000_000_000,
    severity: 'info',
    subsystemTarget: 'provider_transport',
    messageLine: 'provider request completed',
    providerRequestTrace: trace,
  },
  {
    timestampMillisecondsSinceEpoch: 1_700_000_001_000,
    severity: 'warn',
    subsystemTarget: 'provider_transport',
    messageLine: 'provider request warning',
    providerRequestTrace: { ...trace, statusClass: '4xx', outcome: 'client-error' },
  },
];

const frameStatistics: FrameStatisticsEvent = {
  live: true,
  requestedTileCount: 12,
  presentTileCount: 8,
  fallbackTileCount: 2,
  deferredTileCount: 1,
  unavailableTileCount: 1,
  overlayTriangleCount: 40,
  framesDrawnCount: 5,
  framesDeferredCount: 0,
  framesFailedCount: 0,
};

const trafficCacheStatistics: TrafficFlowCacheStatistics = {
  hitCount: 7,
  missCount: 2,
  entryCount: 4,
  evictionCount: 1,
  segmentCount: 30,
  gridZoomLevel: 12,
  maximumEntryCount: 64,
  timeToLiveSeconds: 300,
};

const tileCacheStatistics: TileCacheStatistics = {
  hitCount: 5,
  missCount: 2,
  storedByteCount: 4096,
};

/** The console with every prop the screen supplies. */
function consoleTree(overrides: Partial<React.ComponentProps<typeof DeveloperConsole>> = {}) {
  return (
    <DeveloperConsole
      events={events}
      isOpen={false}
      hasUnreadAlert={false}
      onOpen={jest.fn()}
      onClose={jest.fn()}
      displayNameForProvider={displayNameForProvider}
      tileStatistics={frameStatistics}
      tileCacheStatistics={tileCacheStatistics}
      trafficAvailability={{
        state: 'available',
        providerKey: 'traffic-source',
        attributionText: 'Traffic data by Traffic Source',
      }}
      trafficCacheStatistics={trafficCacheStatistics}
      controlBottomOffset={80}
      styles={styles}
      {...overrides}
    />
  );
}

/** The visible text of one node and its descendants. */
function textContent(node: ReactTestRenderer.ReactTestInstance): string {
  return node
    .findAllByType(Text)
    .map(textNode => {
      const children = textNode.props.children;
      return Array.isArray(children) ? children.join('') : String(children ?? '');
    })
    .join('\n');
}

test('the floating control opens the console and carries a Warning/Error dot', async () => {
  const onOpen = jest.fn();
  let renderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    renderer = ReactTestRenderer.create(
      consoleTree({ onOpen, hasUnreadAlert: true }),
    );
  });
  const root = renderer!.root;

  const control = root.findByProps({ testID: 'developer-console-control' });
  expect(control).toBeDefined();
  expect(
    root.findAllByProps({ testID: 'developer-console-alert-dot' }).length,
  ).toBeGreaterThanOrEqual(1);
  await ReactTestRenderer.act(() => {
    control.props.onPress();
  });
  expect(onOpen).toHaveBeenCalledTimes(1);
});

test('opening renders the injected Info events newest first and filters by level', async () => {
  let renderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    renderer = ReactTestRenderer.create(consoleTree({ isOpen: true }));
  });
  const root = renderer!.root;

  expect(root.findByProps({ testID: 'developer-console' })).toBeDefined();
  const firstRowText = textContent(
    root.findByProps({ testID: 'developer-console-row-0' }),
  );
  expect(firstRowText).toContain('provider request warning');
  expect(firstRowText).toContain('MapQuest');
  expect(firstRowText).toContain('RoutingProvider');
  expect(firstRowText).toContain('GET /directions/v2/route');
  expect(firstRowText).toContain('4xx');
  expect(firstRowText).toContain('123 ms');

  // The Warning filter drops the Info row; the Error filter shows none.
  await ReactTestRenderer.act(() => {
    root.findByProps({ testID: 'developer-console-level-error' }).props.onPress();
  });
  expect(
    root.findAllByProps({ testID: 'developer-console-empty' }).length,
  ).toBeGreaterThanOrEqual(1);
});

test('the empty state is honest when the window holds no level event', async () => {
  let renderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    renderer = ReactTestRenderer.create(consoleTree({ isOpen: true, events: [] }));
  });
  const root = renderer!.root;
  const empty = root.findByProps({ testID: 'developer-console-empty' });
  expect(textContent(empty)).toContain(
    'No events recorded at this level yet.',
  );
  expect(
    root.findAllByProps({ testID: 'developer-console-row-0' }).length,
  ).toBe(0);
});

test('tapping a row opens the typed trace and cache detail', async () => {
  let renderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    renderer = ReactTestRenderer.create(consoleTree({ isOpen: true }));
  });
  const root = renderer!.root;

  await ReactTestRenderer.act(() => {
    root.findByProps({ testID: 'developer-console-row-0' }).props.onPress();
  });

  expect(root.findByProps({ testID: 'developer-console-detail' })).toBeDefined();
  const detailText = textContent(
    root.findByProps({ testID: 'developer-console-detail' }),
  );
  expect(detailText).toContain('Provider key: mapquest');
  expect(detailText).toContain('Contract: RoutingProvider');
  expect(detailText).toContain('Request path: /directions/v2/route');
  expect(detailText).toContain('Status class: 4xx');
  expect(detailText).toContain('Duration: 123 ms');
  expect(detailText).toContain('Response bytes: 4096');
  expect(detailText).toContain('Tile cache: hits 5');
  expect(detailText).toContain('stored 4096 bytes');
  expect(detailText).toContain('Last frame: requested 12');
  expect(detailText).toContain('hits 7');
});

test('export hands the filtered plain-text list to the share sheet', async () => {
  const shareSpy = jest
    .spyOn(Share, 'share')
    .mockResolvedValue({ action: Share.sharedAction, activityType: undefined });
  let renderer: ReactTestRenderer.ReactTestRenderer;
  await ReactTestRenderer.act(() => {
    renderer = ReactTestRenderer.create(consoleTree({ isOpen: true }));
  });
  const root = renderer!.root;

  await ReactTestRenderer.act(async () => {
    root.findByProps({ testID: 'developer-console-export' }).props.onPress();
    await Promise.resolve();
    await Promise.resolve();
  });

  expect(shareSpy).toHaveBeenCalledTimes(1);
  const sharedMessage = shareSpy.mock.calls[0][0].message as string;
  expect(sharedMessage).toContain('provider request warning');
  expect(sharedMessage).toContain('provider request completed');
  expect(sharedMessage).not.toContain('api_key=');
  expect(
    root.findByProps({ testID: 'developer-console-export-status' }),
  ).toBeDefined();
  expect(
    textContent(
      root.findByProps({ testID: 'developer-console-export-status' }),
    ),
  ).toContain(developerConsoleExportSharedText);
  shareSpy.mockRestore();
});
