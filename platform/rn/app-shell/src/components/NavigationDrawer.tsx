import React, { useCallback, useMemo, useState } from 'react';
import { PanResponder, Pressable, Text, View } from 'react-native';

import type {
  PlaybackStateEvent,
  SpeechEngineStateEvent,
  TraceUpdateEvent,
} from '../RefappNative';
import { deviceNavigationStopText } from '../helpers/deviceNavigation';
import { simulatedTripSpeedMultiplierChoices } from '../preferences';
import type { buildScreenStyles } from '../screenStyles';
import type { SpeechConsumerState } from '../speechConsumer';
import { appStrings } from '../theme';

/**
 * The drawer's seek control: one scrubber over the route's own length.
 * A touch or drag reports the fraction of the track it names, and the native
 * walker supplies the fix at that distance, so the camera, the indicator, the
 * ETA line and the arrival follow exactly as on a normal fix. The row stays
 * visible in the arrival state so a backwards seek can leave it.
 */
function NavigationSeekControl(props: {
  labelText: string;
  travelledMeters: number;
  totalMeters: number;
  onSeek: (travelledMeters: number) => void;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  const { onSeek, totalMeters } = props;
  const [trackWidth, setTrackWidth] = useState(0);
  const seekFromTrackPosition = useCallback(
    (trackPosition: number) => {
      if (trackWidth <= 0 || totalMeters <= 0) {
        return;
      }
      const travelledFraction = Math.min(
        Math.max(trackPosition / trackWidth, 0),
        1,
      );
      onSeek(travelledFraction * totalMeters);
    },
    [onSeek, totalMeters, trackWidth],
  );
  const panResponder = useMemo(
    () =>
      PanResponder.create({
        onStartShouldSetPanResponder: () => true,
        onMoveShouldSetPanResponder: () => true,
        onPanResponderGrant: event =>
          seekFromTrackPosition(event.nativeEvent.locationX),
        onPanResponderMove: event =>
          seekFromTrackPosition(event.nativeEvent.locationX),
      }),
    [seekFromTrackPosition],
  );
  const travelledFraction =
    props.totalMeters > 0
      ? Math.min(Math.max(props.travelledMeters / props.totalMeters, 0), 1)
      : 0;
  return (
    <View style={props.styles.seekRow} testID="navigation-seek-row">
      <Text style={props.styles.drawerSpeedLabel}>{props.labelText}</Text>
      <View
        style={props.styles.seekTrack}
        onLayout={layoutEvent =>
          setTrackWidth(layoutEvent.nativeEvent.layout.width)
        }
        accessibilityRole="adjustable"
        accessibilityLabel={props.labelText}
        accessibilityValue={{
          min: 0,
          max: props.totalMeters,
          now: props.travelledMeters,
        }}
        {...panResponder.panHandlers}
        testID="navigation-seek-track">
        <View
          style={[
            props.styles.seekFill,
            { width: `${travelledFraction * 100}%` },
          ]}
        />
      </View>
    </View>
  );
}

/**
 * The navigation drawer (§7.2): the ETA-distance line, phase, the
 * playback-speed row and one Pause/Resume control while the trip runs, the
 * speech toggle (§4.2's row — disabled-with-reason while the engine is
 * starting or failed), the honest last-spoken line the consumer carries, and
 * the exit. An arrival replaces the playback controls with the arrived line:
 * the destination stays presented until Exit, as on the other platforms.
 */
export function NavigationDrawer(props: {
  traceUpdate: TraceUpdateEvent | null;
  playbackState: PlaybackStateEvent;
  /** A device-driven trip has no playback to steer (speed, pause, seek). */
  isDeviceNavigationActive: boolean;
  /** Ends a device-driven trip from the drawer the trip replaces the sheet
   * with, at the same control and label the half sheet carries. */
  onStopDeviceNavigation: () => void;
  safeAreaBottom: number;
  onExit: () => void;
  onTogglePause: () => void;
  onSelectSpeed: (speedMultiplier: number) => void;
  onSeek: (travelledMeters: number) => void;
  speechConsumerState: SpeechConsumerState;
  speechEngineState: SpeechEngineStateEvent;
  onToggleSpeech: (nextSpeechEnabled: boolean) => void;
  onMeasuredHeight: (height: number) => void;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  // The walker and the controller each declare the arrival; either one hides
  // the playback controls (the presentation outlives the playback).
  const hasArrived =
    props.traceUpdate?.isArrival === true || props.playbackState.hasArrived;
  const playbackControlText = props.playbackState.isPaused
    ? appStrings.navigationResumeActionText
    : appStrings.navigationPauseActionText;
  return (
    <View
      style={[props.styles.navigationDrawer, { bottom: props.safeAreaBottom }]}
      onLayout={layoutEvent =>
        props.onMeasuredHeight(layoutEvent.nativeEvent.layout.height)
      }
      testID="navigation-drawer">
      <Text style={props.styles.drawerPhaseLabel} testID="drawer-phase-line">
        {props.traceUpdate?.phase ?? appStrings.phaseNotStartedText}
      </Text>
      <Text style={props.styles.drawerEtaLabel} testID="drawer-eta-line">
        {appStrings.navigationDrawerEtaLabelText}: {props.traceUpdate?.progressLineText ?? ''}
      </Text>
      {hasArrived ? (
        <Text style={props.styles.arrivedText}>{appStrings.arrivedText}</Text>
      ) : props.isDeviceNavigationActive ? null : (
        <View style={props.styles.drawerControlRow} testID="navigation-speed-row">
          <Text style={props.styles.drawerSpeedLabel}>
            {appStrings.navigationSpeedLabelText}
          </Text>
          {simulatedTripSpeedMultiplierChoices.map(speedMultiplier => {
            const isSelected =
              props.playbackState.speedMultiplier === speedMultiplier;
            return (
              <Pressable
                key={speedMultiplier}
                style={[props.styles.chip, isSelected && props.styles.chipActive]}
                onPress={() => props.onSelectSpeed(speedMultiplier)}
                testID={'navigation-speed-chip-' + speedMultiplier}>
                <Text
                  style={[
                    props.styles.chipText,
                    isSelected && props.styles.chipTextActive,
                  ]}>
                  {appStrings.navigationSpeedMultiplierFormat.replace(
                    '{multiplier}',
                    String(speedMultiplier),
                  )}
                </Text>
              </Pressable>
            );
          })}
        </View>
      )}
      {props.isDeviceNavigationActive ? (
        <View style={props.styles.drawerControlRow} testID="drawer-device-navigation-row">
          <Pressable
            style={[props.styles.drawerExitControl, props.styles.chipActive]}
            onPress={props.onStopDeviceNavigation}
            testID="device-navigation-control">
            <Text style={props.styles.drawerExitControlText}>
              {deviceNavigationStopText}
            </Text>
          </Pressable>
        </View>
      ) : null}
      {props.playbackState.isSeekable ? (
        <NavigationSeekControl
          labelText={appStrings.navigationSeekLabelText}
          travelledMeters={props.traceUpdate?.travelledMeters ?? 0}
          totalMeters={props.playbackState.totalMeters}
          onSeek={props.onSeek}
          styles={props.styles}
        />
      ) : null}
      {props.speechEngineState.isEngineReady ? (
        <>
          <View style={props.styles.drawerControlRow} testID="speech-row">
            <Pressable
              style={[
                props.styles.chip,
                props.speechConsumerState.isSpeechEnabled && props.styles.chipActive,
              ]}
              onPress={() => props.onToggleSpeech(!props.speechConsumerState.isSpeechEnabled)}
              testID="speech-toggle">
              <Text
                style={[
                  props.styles.chipText,
                  props.speechConsumerState.isSpeechEnabled && props.styles.chipTextActive,
                ]}>
                {appStrings.speechToggleLabelText}
              </Text>
            </Pressable>
            {hasArrived || props.isDeviceNavigationActive ? null : (
              <Pressable
                style={props.styles.drawerExitControl}
                onPress={props.onTogglePause}
                testID="drawer-playback-pause">
                <Text style={props.styles.drawerExitControlText}>
                  {playbackControlText}
                </Text>
              </Pressable>
            )}
            <Pressable
              style={props.styles.drawerExitControl}
              onPress={props.onExit}
              testID="drawer-exit">
              <Text style={props.styles.drawerExitControlText}>
                {appStrings.navigationDrawerExitButtonText}
              </Text>
            </Pressable>
          </View>
          <Text style={props.styles.drawerSpeechLine} testID="drawer-speech-line">
            {appStrings.speechToggleLabelText}: {props.speechConsumerState.lastSpokenText ?? ''}
          </Text>
        </>
      ) : (
        <View style={props.styles.drawerControlRow} testID="speech-row">
          <Text style={props.styles.drawerSpeechReason} testID="drawer-speech-reason">
            {appStrings.speechToggleLabelText} {appStrings.speechToggleUnavailableLeadInText}{' '}
            {props.speechEngineState.unavailableReasonText ?? appStrings.speechEngineStartingText}
          </Text>
          {hasArrived ? null : (
            <Pressable
              style={props.styles.drawerExitControl}
              onPress={props.onTogglePause}
              testID="drawer-playback-pause">
              <Text style={props.styles.drawerExitControlText}>
                {playbackControlText}
              </Text>
            </Pressable>
          )}
          <Pressable
            style={props.styles.drawerExitControl}
            onPress={props.onExit}
            testID="drawer-exit">
            <Text style={props.styles.drawerExitControlText}>
              {appStrings.navigationDrawerExitButtonText}
            </Text>
          </Pressable>
        </View>
      )}
    </View>
  );
}
