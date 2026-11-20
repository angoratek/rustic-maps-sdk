import React from 'react';
import { Pressable, Text, View } from 'react-native';

import type { PlanStateEvent } from '../RefappNative';
import type { buildScreenStyles } from '../screenStyles';
import { appStrings } from '../theme';

/** The route summary card (§4.2's distance·duration, Start and Edit). */
export function RouteSummaryCard(props: {
  planState: PlanStateEvent;
  styles: ReturnType<typeof buildScreenStyles>;
  onStart: () => void;
  onEdit: () => void;
}) {
  return (
    <View style={props.styles.routeSummaryCard} testID="route-summary-card">
      {/* The live route card carries its facts, not a destination title
          (the iOS card shows no destination title); only the offline
          archive's own title names the test graph it came from. */}
      {props.planState.isOfflineArchiveRoute ? (
        <Text style={props.styles.routeSummaryTitle} testID="route-summary-title">
          {appStrings.offlineRouteTitleText}
        </Text>
      ) : null}
      <Text style={props.styles.routeSummaryMeta}>
        {props.planState.isInFlight
          ? appStrings.routeSummaryPlanningText
          : `${props.planState.distanceText ?? ''} · ${props.planState.durationText ?? ''}`}
      </Text>
      {props.planState.isRecordedDemoRoute ? (
        <Text style={props.styles.routeSummaryMeta} testID="route-summary-recorded-note">
          {appStrings.recordedDemoRouteNoteText}
        </Text>
      ) : null}
      {props.planState.isOfflineArchiveRoute ? (
        <Text style={props.styles.routeSummaryMeta} testID="route-summary-offline-note">
          {appStrings.offlineRouteNoteText}
        </Text>
      ) : null}
      <View style={props.styles.peekActionRow}>
        <Pressable
          style={[props.styles.actionControl, props.styles.actionControlPrimary]}
          onPress={props.onStart}
          testID="route-summary-start">
          <Text style={[props.styles.actionControlText, props.styles.actionControlTextPrimary]}>
            {appStrings.routeSummaryStartButtonText}
          </Text>
        </Pressable>
        <Pressable style={props.styles.actionControl} onPress={props.onEdit} testID="route-summary-edit">
          <Text style={props.styles.actionControlText}>{appStrings.routeSummaryEditButtonText}</Text>
        </Pressable>
      </View>
    </View>
  );
}
