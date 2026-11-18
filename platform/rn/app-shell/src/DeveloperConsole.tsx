/**
 * The developer console surface (M58): a floating control on the edge opposite
 * the map layer control, and a sheet reading the scrubbed SDK diagnostics
 * stream at Error, Warning and Info.
 *
 * The sheet lists one row per event, newest first, filtered by the selected
 * minimum level. A row built from a typed provider request trace names the
 * provider, the contract, the method, the redacted path, the status class, the
 * duration and a truncated response excerpt. Tapping a row opens its detail:
 * every typed trace field plus the tile and traffic cache summary. Export
 * hands the filtered plain-text list to the platform share sheet; the trace is
 * redacted at emission, so a credential cannot appear in the export.
 *
 * The surface is read-only over the SDK stream: it never clears the ring and
 * never issues a provider call.
 */

import React, { useMemo, useState } from 'react';
import {
  Pressable,
  ScrollView,
  Share,
  Text,
  View,
} from 'react-native';

import type { FrameStatisticsEvent } from './RefappNative';
import type { TrafficLayerAvailability } from './mapLayersPanelState';
import type {
  TileCacheStatistics,
  TrafficFlowCacheStatistics,
} from 'maps-sdk-rn/MapsSdk';
import type { DiagnosticEvent } from './diagnosticsStream';
import {
  buildProviderConsoleRows,
  providerConsoleCacheLines,
  providerConsoleDetailLines,
  providerConsoleEmptyText,
  providerConsoleExportText,
  providerConsoleLevelLabels,
  providerConsoleLevelOrder,
  providerConsoleRowLine,
  providerConsoleTimeText,
  type ProviderConsoleLevel,
  type ProviderConsoleRow,
} from './providerConsole';
import type { buildScreenStyles } from './screenStyles';
import { appStrings } from './theme';

/** The control's accessible name. */
export const developerConsoleControlAccessibilityText =
  'Open developer console';

/** The sheet's title and export action words. */
export const developerConsoleTitleText = 'Developer console';
export const developerConsoleExportActionText = 'Export';
export const developerConsoleBackActionText = 'Back';

/** The sheet's section headings. */
export const developerConsoleLevelSectionTitleText = 'Level';
export const developerConsoleCacheSectionTitleText = 'Cache';

/** The export's honest outcome lines. */
export const developerConsoleExportSharedText = 'Filtered list shared.';
export const developerConsoleExportDismissedText = 'Export dismissed.';

/** What the console renders and drives. */
export interface DeveloperConsoleProps {
  /** The diagnostics window, oldest first. */
  events: readonly DiagnosticEvent[];
  /** Whether the sheet is open. */
  isOpen: boolean;
  /** Whether a Warning or Error arrived since the console last opened. */
  hasUnreadAlert: boolean;
  /** Opens the sheet. */
  onOpen: () => void;
  /** Closes the sheet. */
  onClose: () => void;
  /** The registry display name for a provider key. */
  displayNameForProvider: (providerKey: string) => string;
  /** The session's last settled frame, or null before the first frame. */
  tileStatistics: FrameStatisticsEvent | null;
  /** The installed tile payload cache's exact counters, or null when none. */
  tileCacheStatistics: TileCacheStatistics | null;
  /** The traffic layer's registry-derived availability. */
  trafficAvailability: TrafficLayerAvailability;
  /** The serving traffic cache's counters, or null when none are reported. */
  trafficCacheStatistics: TrafficFlowCacheStatistics | null;
  /** The floating control's bottom offset above the attribution notice. */
  controlBottomOffset: number;
  /** The screen's themed styles. */
  styles: ReturnType<typeof buildScreenStyles>;
}

/**
 * The floating control plus the console sheet. It keeps only the filter and
 * the selected row: the events themselves belong to the diagnostics stream.
 */
export function DeveloperConsole(props: DeveloperConsoleProps): React.JSX.Element {
  const [level, setLevel] = useState<ProviderConsoleLevel>('info');
  const [selectedRowKey, setSelectedRowKey] = useState<string | null>(null);
  const [exportStatusText, setExportStatusText] = useState<string | null>(null);

  const rows = useMemo(
    () =>
      buildProviderConsoleRows(
        props.events,
        level,
        props.displayNameForProvider,
      ),
    [props.events, props.displayNameForProvider, level],
  );
  const selectedRow: ProviderConsoleRow | null =
    selectedRowKey === null
      ? null
      : rows.find(row => row.rowKey === selectedRowKey) ?? null;
  const cacheLines = useMemo(
    () =>
      providerConsoleCacheLines({
        tileStatistics: props.tileStatistics,
        tileCacheStatistics: props.tileCacheStatistics,
        trafficAvailability: props.trafficAvailability,
        trafficCacheStatistics: props.trafficCacheStatistics,
        displayNameForProvider: props.displayNameForProvider,
      }),
    [
      props.tileStatistics,
      props.tileCacheStatistics,
      props.trafficAvailability,
      props.trafficCacheStatistics,
      props.displayNameForProvider,
    ],
  );

  const closeSheet = () => {
    setSelectedRowKey(null);
    setExportStatusText(null);
    props.onClose();
  };

  const selectLevel = (nextLevel: ProviderConsoleLevel) => {
    setLevel(nextLevel);
    setSelectedRowKey(null);
  };

  const exportRows = async () => {
    const exportText = providerConsoleExportText(rows);
    try {
      const result = await Share.share({
        title: developerConsoleTitleText,
        message: exportText,
      });
      setExportStatusText(
        result.action === Share.dismissedAction
          ? developerConsoleExportDismissedText
          : developerConsoleExportSharedText,
      );
    } catch (exportError) {
      setExportStatusText(`Export failed: ${String(exportError)}`);
    }
  };

  return (
    <>
      {!props.isOpen ? (
        <Pressable
          style={[
            props.styles.developerConsoleControl,
            { bottom: props.controlBottomOffset },
          ]}
          onPress={props.onOpen}
          accessibilityRole="button"
          accessibilityLabel={developerConsoleControlAccessibilityText}
          testID="developer-console-control">
          <Text style={props.styles.developerConsoleControlText}>{'>_'}</Text>
          {props.hasUnreadAlert ? (
            <View
              style={props.styles.developerConsoleAlertDot}
              testID="developer-console-alert-dot"
            />
          ) : null}
        </Pressable>
      ) : null}

      {props.isOpen ? (
        <View style={props.styles.developerConsoleScrim} testID="developer-console">
          <Pressable
            style={props.styles.developerConsoleBackdrop}
            onPress={closeSheet}
          />
          <View style={props.styles.developerConsoleSheet}>
            <View style={props.styles.developerConsoleHeaderRow}>
              <Text style={props.styles.developerConsoleTitle}>
                {developerConsoleTitleText}
              </Text>
              <Pressable
                style={props.styles.retryControl}
                onPress={closeSheet}
                accessibilityRole="button"
                testID="developer-console-close">
                <Text style={props.styles.retryControlText}>
                  {appStrings.settingsDoneText}
                </Text>
              </Pressable>
            </View>

            {selectedRow === null ? (
              <>
                <Text style={props.styles.developerConsoleSectionHeader}>
                  {developerConsoleLevelSectionTitleText}
                </Text>
                <View
                  style={props.styles.developerConsoleFilterRow}
                  testID="developer-console-level-filter">
                  {providerConsoleLevelOrder.map(levelOption => {
                    const isSelected = levelOption === level;
                    return (
                      <Pressable
                        key={levelOption}
                        style={[
                          props.styles.developerConsoleFilterChip,
                          isSelected
                            ? props.styles.developerConsoleFilterChipActive
                            : null,
                        ]}
                        onPress={() => selectLevel(levelOption)}
                        accessibilityRole="button"
                        accessibilityState={{ selected: isSelected }}
                        testID={`developer-console-level-${levelOption}`}>
                        <Text
                          style={[
                            props.styles.developerConsoleFilterChipText,
                            isSelected
                              ? props.styles.developerConsoleFilterChipTextActive
                              : null,
                          ]}>
                          {providerConsoleLevelLabels[levelOption]}
                        </Text>
                      </Pressable>
                    );
                  })}
                </View>
                <ScrollView
                  style={props.styles.developerConsoleList}
                  contentContainerStyle={props.styles.developerConsoleListContent}
                  testID="developer-console-list">
                  {rows.length === 0 ? (
                    <Text
                      style={props.styles.developerConsoleEmptyText}
                      testID="developer-console-empty">
                      {providerConsoleEmptyText}
                    </Text>
                  ) : (
                    rows.map((row, rowIndex) => (
                      <Pressable
                        key={row.rowKey}
                        style={props.styles.developerConsoleRow}
                        onPress={() => {
                          setSelectedRowKey(row.rowKey);
                          setExportStatusText(null);
                        }}
                        accessibilityRole="button"
                        testID={`developer-console-row-${rowIndex}`}>
                        <Text style={props.styles.developerConsoleRowText}>
                          {providerConsoleRowLine(row)}
                        </Text>
                        {row.truncatedResponseExcerpt !== null ? (
                          <Text
                            style={props.styles.developerConsoleRowExcerpt}
                            testID={`developer-console-row-excerpt-${rowIndex}`}>
                            {row.truncatedResponseExcerpt}
                          </Text>
                        ) : null}
                      </Pressable>
                    ))
                  )}
                </ScrollView>
              </>
            ) : (
              <ScrollView
                style={props.styles.developerConsoleDetail}
                testID="developer-console-detail">
                <Pressable
                  style={props.styles.retryControl}
                  onPress={() => setSelectedRowKey(null)}
                  accessibilityRole="button"
                  testID="developer-console-detail-back">
                  <Text style={props.styles.retryControlText}>
                    {developerConsoleBackActionText}
                  </Text>
                </Pressable>
                <Text
                  style={props.styles.developerConsoleSectionHeader}
                  testID="developer-console-detail-timestamp">
                  {`${providerConsoleTimeText(
                    selectedRow.event.timestampMillisecondsSinceEpoch,
                  )} · ${selectedRow.levelLabel}`}
                </Text>
                {providerConsoleDetailLines(selectedRow).map((line, lineIndex) => (
                  <Text
                    key={`detail-${lineIndex}`}
                    style={props.styles.developerConsoleDetailLine}
                    testID={`developer-console-detail-line-${lineIndex}`}>
                    {line}
                  </Text>
                ))}
                <Text style={props.styles.developerConsoleSectionHeader}>
                  {developerConsoleCacheSectionTitleText}
                </Text>
                {cacheLines.map((line, lineIndex) => (
                  <Text
                    key={`cache-${lineIndex}`}
                    style={props.styles.developerConsoleDetailLine}
                    testID={`developer-console-cache-line-${lineIndex}`}>
                    {line}
                  </Text>
                ))}
              </ScrollView>
            )}

            <Text style={props.styles.developerConsoleSectionHeader}>
              {developerConsoleCacheSectionTitleText}
            </Text>
            <View testID="developer-console-cache-summary">
              {cacheLines.map((line, lineIndex) => (
                <Text
                  key={`footer-cache-${lineIndex}`}
                  style={props.styles.developerConsoleStatusText}
                  testID={`developer-console-footer-cache-${lineIndex}`}>
                  {line}
                </Text>
              ))}
            </View>

            <View style={props.styles.developerConsoleFilterRow}>
              <Pressable
                style={props.styles.retryControl}
                onPress={() => {
                  exportRows().catch(() => undefined);
                }}
                accessibilityRole="button"
                testID="developer-console-export">
                <Text style={props.styles.retryControlText}>
                  {developerConsoleExportActionText}
                </Text>
              </Pressable>
              {exportStatusText !== null ? (
                <Text
                  style={props.styles.developerConsoleStatusText}
                  testID="developer-console-export-status">
                  {exportStatusText}
                </Text>
              ) : null}
            </View>
          </View>
        </View>
      ) : null}
    </>
  );
}
