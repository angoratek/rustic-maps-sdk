import React, { useCallback, useEffect, useRef } from 'react';
import { Pressable, Text, View } from 'react-native';

import { mapControlZoomRepeatIntervalMilliseconds } from '../mapControls';
import type { buildScreenStyles } from '../screenStyles';
import { appStrings } from '../theme';

/**
 * The map control cluster (§4.2): zoom −/+, my-location and the 2D/3D plane
 * toggle floating at the map's end edge, vertically centered like the Compose
 * reference application's `Alignment.CenterEnd` cluster. The zoom controls
 * repeat their step while held at the §2.4 cadence; a plain tap steps once.
 */
export function MapControlCluster(props: {
  isThreeDCameraEnabled: boolean;
  isRecentering: boolean;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onMyLocation: () => void;
  onTogglePlane: () => void;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  return (
    <View
      style={props.styles.mapControlCluster}
      pointerEvents="box-none"
      testID="map-control-cluster">
      <HoldRepeatControl
        label={appStrings.mapControlZoomInSymbol}
        accessibilityLabel={appStrings.mapControlZoomInAccessibilityText}
        onActivated={props.onZoomIn}
        styles={props.styles}
        testID="map-control-zoom-in"
      />
      <HoldRepeatControl
        label={appStrings.mapControlZoomOutSymbol}
        accessibilityLabel={appStrings.mapControlZoomOutAccessibilityText}
        onActivated={props.onZoomOut}
        styles={props.styles}
        testID="map-control-zoom-out"
      />
      <Pressable
        style={[
          props.styles.mapControlButton,
          props.isRecentering && props.styles.mapControlButtonActive,
        ]}
        onPress={props.onMyLocation}
        accessibilityRole="button"
        accessibilityLabel={
          props.isRecentering
            ? appStrings.mapControlRecenterAccessibilityText
            : appStrings.mapControlMyLocationAccessibilityText
        }
        testID="map-control-my-location">
        <View
          style={[
            props.styles.myLocationGlyphRing,
            props.isRecentering && props.styles.myLocationGlyphRingActive,
          ]}>
          <View
            style={[
              props.styles.myLocationGlyphDot,
              props.isRecentering && props.styles.myLocationGlyphDotActive,
            ]}
          />
        </View>
      </Pressable>
      <Pressable
        style={props.styles.mapControlButton}
        onPress={props.onTogglePlane}
        accessibilityRole="button"
        accessibilityLabel={appStrings.mapControlPlaneAccessibilityText}
        testID="map-control-plane">
        <Text
          style={[
            props.styles.mapControlButtonText,
            props.isThreeDCameraEnabled && props.styles.mapControlButtonTextActive,
          ]}>
          {props.isThreeDCameraEnabled
            ? appStrings.mapControlThreeDLabelText
            : appStrings.mapControlTwoDLabelText}
        </Text>
      </Pressable>
    </View>
  );
}

/**
 * One hold-to-repeat control (the zoom buttons): the activation fires on the
 * press and repeats at the §2.4 cadence until release, so a hold never
 * double-steps on the release itself.
 */
function HoldRepeatControl(props: {
  label: string;
  accessibilityLabel: string;
  onActivated: () => void;
  styles: ReturnType<typeof buildScreenStyles>;
  testID: string;
}) {
  const repeatTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const stopRepeating = useCallback(() => {
    if (repeatTimerRef.current !== null) {
      clearInterval(repeatTimerRef.current);
      repeatTimerRef.current = null;
    }
  }, []);
  useEffect(() => stopRepeating, [stopRepeating]);
  const startRepeating = useCallback(() => {
    props.onActivated();
    stopRepeating();
    repeatTimerRef.current = setInterval(
      props.onActivated,
      mapControlZoomRepeatIntervalMilliseconds,
    );
  }, [props.onActivated, stopRepeating]);
  return (
    <Pressable
      style={props.styles.mapControlButton}
      onPressIn={startRepeating}
      onPressOut={stopRepeating}
      accessibilityRole="button"
      accessibilityLabel={props.accessibilityLabel}
      testID={props.testID}>
      <Text style={props.styles.mapControlButtonText}>{props.label}</Text>
    </Pressable>
  );
}
