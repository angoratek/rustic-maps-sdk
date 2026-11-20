/**
 * The compass control (§4.2): top-end, a north-up/course-up mode toggle
 * during a trip.
 */

import React from 'react';
import { Pressable, Text } from 'react-native';

import {
  bearingOffNorthDegrees,
  compassVisibilityBearingThresholdDegrees,
  mapControlLayout,
} from '../mapControls';
import type { buildScreenStyles } from '../screenStyles';
import { appStrings, themeSpacing } from '../theme';

/**
 * The compass control (§4.2): top-end. During a trip it is a mode toggle
 * (M73): a tap chooses north-up and the control stays visible while that mode
 * holds, so a second tap can resume course-up. Outside a trip it appears only
 * while the camera is off north by the §2.4 threshold.
 */
export function CompassControl(props: {
  bearingDegrees: number;
  traceActive: boolean;
  isHoldingNorthUp: boolean;
  safeAreaTop: number;
  onPress: () => void;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  if (
    !props.isHoldingNorthUp &&
    bearingOffNorthDegrees(props.bearingDegrees) <
      compassVisibilityBearingThresholdDegrees
  ) {
    return null;
  }
  const topClearance = props.traceActive
    ? mapControlLayout.compassNavigationTopClearance
    : themeSpacing.stackGroup;
  return (
    <Pressable
      style={[props.styles.compassButton, { top: props.safeAreaTop + topClearance }]}
      onPress={props.onPress}
      accessibilityRole="button"
      accessibilityLabel={appStrings.mapControlCompassAccessibilityText}
      testID="compass-control">
      <Text
        style={[
          props.styles.compassLetter,
          { transform: [{ rotate: `${-props.bearingDegrees}deg` }] },
        ]}>
        {appStrings.mapControlCompassNorthLetter}
      </Text>
    </Pressable>
  );
}
