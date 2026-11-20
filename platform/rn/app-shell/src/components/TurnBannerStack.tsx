/**
 * The turn banner stack above the map during a demo (§7.1).
 */

import React from 'react';
import { Text, View } from 'react-native';

import type { TraceUpdateEvent } from '../RefappNative';
import type { buildScreenStyles } from '../screenStyles';
import { appStrings } from '../theme';

/** The banner stack's own top gap below the status bar (§7.1's stacking). */
const bannerStackGap = 56;

export function TurnBannerStack(props: {
  traceActive: boolean;
  traceUpdate: TraceUpdateEvent | null;
  deviationText: string | null;
  safeAreaTop: number;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  if (!props.traceActive) {
    return null;
  }
  return (
    <View
      style={[props.styles.bannerStack, { top: props.safeAreaTop + bannerStackGap }]}
      pointerEvents="none"
      testID="turn-banner">
      {props.deviationText ? (
        <View style={[props.styles.bannerCard, props.styles.deviationCard]}>
          <Text style={props.styles.deviationText}>{props.deviationText}</Text>
        </View>
      ) : null}
      {props.traceUpdate?.isArrival ? (
        <View style={props.styles.bannerCard}>
          <Text style={props.styles.arrivedText}>{appStrings.arrivedText}</Text>
        </View>
      ) : (
        <>
          <View style={props.styles.bannerCard}>
            <Text style={props.styles.bannerHeadline}>
              {props.traceUpdate?.bannerHeadlineText ?? ''}
            </Text>
            <Text style={props.styles.bannerBody}>
              {props.traceUpdate?.bannerDisplayText ?? appStrings.awaitingGuidanceText}
            </Text>
          </View>
          {props.traceUpdate?.upcomingDisplayText ? (
            <View style={props.styles.bannerCard}>
              <Text style={props.styles.bannerUpcoming}>{props.traceUpdate.upcomingDisplayText}</Text>
            </View>
          ) : null}
        </>
      )}
    </View>
  );
}
