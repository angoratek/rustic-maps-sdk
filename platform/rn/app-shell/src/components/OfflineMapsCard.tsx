import React from 'react';
import { Pressable, Text, View } from 'react-native';

import type { CacheStateEvent, FrameStatisticsEvent } from '../RefappNative';
import type {
  RegionDownloadProgress,
  RegionRecord,
} from '@angoratek/rustic-maps-sdk/MapsSdk';
import type { buildScreenStyles } from '../screenStyles';
import { appStrings } from '../theme';

/** The in-flight region download's honest count line. */
function offlineRegionProgressText(progress: RegionDownloadProgress): string {
  return (
    `${progress.completedTileCount}/${progress.totalTileCount} · ` +
    `${progress.networkFetchedTileCount} network · ` +
    `${progress.diskServedTileCount} disk`
  );
}

/**
 * The offline maps card: the registered cache line, the live drawn-tile line,
 * the demo region download (progress, cancel and resume) and the stored
 * regions with their tile and byte counts and Delete controls.
 */
export function OfflineMapsCard(props: {
  cacheState: CacheStateEvent | null;
  tileStatistics: FrameStatisticsEvent | null;
  isOfflineRouteInFlight: boolean;
  onPlanOfflineRoute: () => void;
  demoRegionNameText: string | null;
  demoRegionTileCount: number | null;
  isOfflineCacheRegistered: boolean;
  regionProgress: RegionDownloadProgress | null;
  regionStatus: 'idle' | 'downloading' | 'completed' | 'cancelled' | 'failed';
  regionErrorText: string | null;
  isRegionDownloadInFlight: boolean;
  storedRegions: RegionRecord[];
  isRegionDeleted: boolean;
  onDownloadRegion: () => void;
  onCancelRegionDownload: () => void;
  onDeleteRegion: (regionId: string) => void;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  const cacheLine = props.cacheState?.stateText ?? 'Cache not registered';
  const tileLine = props.tileStatistics?.live
    ? `Tiles requested ${props.tileStatistics.requestedTileCount ?? 0} · ` +
      `drawn ${props.tileStatistics.presentTileCount ?? 0} · ` +
      `fallback ${props.tileStatistics.fallbackTileCount ?? 0}`
    : appStrings.offlineTilesUnavailableText;
  return (
    <View style={props.styles.offlineCard} testID="offline-card">
      <Text style={props.styles.offlineCardTitle}>{appStrings.offlineMapsCardTitleText}</Text>
      <Text style={props.styles.offlineLine} testID="offline-cache-line">
        {cacheLine}
      </Text>
      <Text style={props.styles.offlineLine} testID="offline-tile-line">
        {tileLine}
      </Text>
      {props.demoRegionNameText !== null ? (
        <View style={props.styles.offlineStoredRegion} testID="offline-region">
          <Text style={props.styles.offlineRegionName}>
            {props.demoRegionNameText}
          </Text>
          {props.demoRegionTileCount !== null ? (
            <Text style={props.styles.offlineLine} testID="offline-region-tile-count">
              {`${props.demoRegionTileCount} ${appStrings.offlineRegionTileUnitText}`}
            </Text>
          ) : null}
          {props.regionStatus === 'downloading' ? (
            <Text style={props.styles.offlineLine} testID="offline-region-progress">
              {appStrings.offlineDownloadingStatusText}
              {props.regionProgress !== null
                ? ` ${offlineRegionProgressText(props.regionProgress)}`
                : ''}
            </Text>
          ) : null}
          {props.regionStatus === 'cancelled' &&
          props.regionProgress !== null ? (
            <Text style={props.styles.offlineLine} testID="offline-region-cancelled">
              {offlineRegionProgressText(props.regionProgress)}
            </Text>
          ) : null}
          {props.regionStatus === 'completed' ? (
            <Text style={props.styles.offlineLine} testID="offline-region-completed">
              {appStrings.offlineCompletedStatusText}
            </Text>
          ) : null}
          {props.regionStatus === 'failed' ? (
            <Text style={props.styles.offlineLine} testID="offline-region-failed">
              {appStrings.offlineFailedStatusText}
              {props.regionErrorText !== null ? `: ${props.regionErrorText}` : ''}
            </Text>
          ) : null}
          {props.isRegionDownloadInFlight ? (
            <Pressable
              style={props.styles.actionControl}
              onPress={props.onCancelRegionDownload}
              testID="offline-region-cancel">
              <Text style={props.styles.actionControlText}>
                {appStrings.offlineCancelActionText}
              </Text>
            </Pressable>
          ) : (
            <Pressable
              style={[
                props.styles.actionControl,
                props.isOfflineCacheRegistered
                  ? null
                  : props.styles.actionControlDisabled,
              ]}
              disabled={!props.isOfflineCacheRegistered}
              onPress={props.onDownloadRegion}
              testID="offline-region-download">
              <Text style={props.styles.actionControlText}>
                {appStrings.offlineDownloadActionText}
              </Text>
            </Pressable>
          )}
        </View>
      ) : null}
      {props.storedRegions.map(storedRegion => (
        <View
          key={storedRegion.regionId}
          style={props.styles.offlineStoredRegion}
          testID={`offline-stored-${storedRegion.regionId}`}>
          <Text style={props.styles.offlineRegionName}>
            {storedRegion.displayName}
          </Text>
          <Text style={props.styles.offlineLine}>
            {`${appStrings.offlineStoredTilesLabelText}: ` +
              `${storedRegion.presentTileCount}/${storedRegion.memberTileCount}`}
          </Text>
          <Text style={props.styles.offlineLine}>
            {`${appStrings.offlineStoredBytesLabelText}: ` +
              `${storedRegion.presentPayloadBytes} B`}
          </Text>
          <Pressable
            style={props.styles.actionControl}
            onPress={() => props.onDeleteRegion(storedRegion.regionId)}
            testID={`offline-delete-${storedRegion.regionId}`}>
            <Text style={props.styles.actionControlText}>
              {appStrings.offlineDeleteActionText}
            </Text>
          </Pressable>
        </View>
      ))}
      {props.isRegionDeleted ? (
        <Text style={props.styles.offlineLine} testID="offline-region-deleted">
          {appStrings.offlineDeletedStatusText}
        </Text>
      ) : null}
      <Pressable
        style={[
          props.styles.actionControl,
          props.isOfflineRouteInFlight ? props.styles.actionControlDisabled : null,
        ]}
        disabled={props.isOfflineRouteInFlight}
        onPress={props.onPlanOfflineRoute}
        testID="offline-route-button">
        <Text style={props.styles.actionControlText}>
          {appStrings.offlineRouteActionText}
        </Text>
      </Pressable>
    </View>
  );
}
