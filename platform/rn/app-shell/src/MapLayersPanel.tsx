/**
 * The on-map layer control (docs/design/MapControlsAndTrafficLayer.md): the collapsed
 * round control docked above the attribution notice, and the panel it opens —
 * Base map, Traffic, Overlays and Device location, in that order.
 *
 * The panel renders the shared registry's descriptors and the shared demo
 * scenes through its props; it names no provider, carries no credential and
 * invents no scene. Every decision lives in `mapLayersPanel.ts`; this file
 * only presents the state it is given.
 */

import React, {
  forwardRef,
  type ReactNode,
} from 'react';
import {
  Modal,
  Pressable,
  ScrollView,
  Switch,
  Text,
  View,
} from 'react-native';

import {
  baseRowAccessibilityLabel,
  baseRowStatusText,
  deviceLocationCannotStartReasonText,
  deviceLocationPanelStatusText,
  deviceLocationStatusIdentifier,
  deviceLocationStopIdentifier,
  isDeviceLocationSourceRunning,
  mapLayerClearOverlaysIdentifier,
  mapLayerTrafficIdentifier,
  mapLayersActiveDotAccessibilityValue,
  mapLayersActiveDotIdentifier,
  mapLayersCloseIdentifier,
  mapLayersControlGlyph,
  mapLayersControlIdentifier,
  mapLayersControlLabel,
  mapLayersPanelIdentifier,
  mapLayersPanelSectionTitles,
  mapLayersStatusIdentifier,
  mapLayersVectorStylesGroupTitle,
  trafficLayerReasonText,
  type DeviceLocationPanelStatus,
  type MapLayerBaseRow,
  type MapLayerSceneRow,
  type TrafficLayerAvailability,
} from './mapLayersPanelState';
import type { buildScreenStyles } from './screenStyles';
import { appStrings, themeSpacing } from './theme';

/** The screen stylesheet the control and panel render with. */
type ScreenStyles = ReturnType<typeof buildScreenStyles>;

/** The collapsed control's host ref (the focus target on close). */
export type MapLayersControlRef = React.ComponentRef<typeof View>;

/** The collapsed control's props. */
export interface MapLayersControlProps {
  /** Whether a non-default layer is active (the control's dot). */
  isNonDefaultLayerActive: boolean;
  onPress: () => void;
  styles: ScreenStyles;
}

/**
 * The collapsed round control: it opens the layer panel and carries a dot
 * while a non-default layer is active. Its ref is the focus target the panel
 * returns to when it closes.
 */
export const MapLayersControl = forwardRef<
  MapLayersControlRef,
  MapLayersControlProps
>(
  function MapLayersControl(props, ref): React.JSX.Element {
    return (
      <Pressable
        ref={ref}
        style={props.styles.mapLayersControl}
        onPress={props.onPress}
        accessibilityRole="button"
        accessibilityLabel={mapLayersControlLabel}
        accessibilityValue={{
          text: props.isNonDefaultLayerActive
            ? mapLayersActiveDotAccessibilityValue
            : '',
        }}
        testID={mapLayersControlIdentifier}>
        <Text style={props.styles.mapLayersControlGlyph}>
          {mapLayersControlGlyph}
        </Text>
        {props.isNonDefaultLayerActive ? (
          <View
            style={props.styles.mapLayersActiveDot}
            accessibilityElementsHidden
            importantForAccessibility="no-hide-descendants"
            testID={mapLayersActiveDotIdentifier}
          />
        ) : null}
      </Pressable>
    );
  },
);

/** The expanded panel's props. */
export interface MapLayersPanelProps {
  /** The base-map rows, in the specification's order. */
  baseRows: readonly MapLayerBaseRow[];
  /** One base row's act. */
  onSelectBaseRow: (row: MapLayerBaseRow) => void;
  /** The traffic switch's availability. */
  trafficAvailability: TrafficLayerAvailability;
  /** Whether the traffic layer is on. */
  isTrafficEnabled: boolean;
  /** The traffic switch's act. */
  onToggleTraffic: (isEnabled: boolean) => void;
  /** One switch per shared demo scene. */
  sceneRows: readonly MapLayerSceneRow[];
  /** Whether the scene overlays can draw (the live surface is ready). */
  areSceneOverlaysAvailable: boolean;
  /** The disabled-with-reason line while the scene overlays cannot draw. */
  sceneOverlaysUnavailableReasonText: string | null;
  /** One scene switch's act. */
  onToggleScene: (sceneKey: string, isEnabled: boolean) => void;
  /** Turns every demo scene off and removes its overlay in one act. */
  onClearScenes: () => void;
  /** The device-location row's state. */
  locationStatus: DeviceLocationPanelStatus;
  /** The row's Stop action while the source runs. */
  onStopDeviceLocation: () => void;
  /** The panel's status line, or null while nothing needs saying. */
  statusLineText: string | null;
  /** The bottom safe-area inset the panel clears, in density-independent units. */
  bottomInset: number;
  /** Closes the panel (its button and the platform's back/escape path). */
  onClose: () => void;
  /** Returns accessibility focus to the collapsed control. */
  onReturnFocus?: () => void;
  styles: ScreenStyles;
}

/**
 * The expanded layer panel. The panel is a modal so the platform's back
 * gesture and Escape close it through `onRequestClose`, and every close path
 * returns focus to the collapsed control.
 */
export function MapLayersPanel(
  props: MapLayersPanelProps,
): React.JSX.Element {
  const handleClose = () => {
    props.onClose();
    props.onReturnFocus?.();
  };
  const trafficReasonText = trafficLayerReasonText(props.trafficAvailability);
  const deviceLocationReasonText = deviceLocationCannotStartReasonText(
    props.locationStatus,
  );

  return (
    <Modal
      visible
      transparent
      animationType="none"
      onRequestClose={handleClose}>
      <View
        style={[
          props.styles.mapLayersModalBackdrop,
          { paddingBottom: props.bottomInset + themeSpacing.gutterBlock },
        ]}>
        <View
          style={props.styles.mapLayersPanel}
          testID={mapLayersPanelIdentifier}
          accessibilityViewIsModal>
          <View style={props.styles.mapLayersHeaderRow}>
            <Text style={props.styles.mapLayersTitle}>
              {mapLayersControlLabel}
            </Text>
            <Pressable
              style={props.styles.mapLayersCloseButton}
              onPress={handleClose}
              accessibilityRole="button"
              testID={mapLayersCloseIdentifier}>
              <Text style={props.styles.mapLayersCloseText}>
                {appStrings.settingsDoneText}
              </Text>
            </Pressable>
          </View>
          <ScrollView
            style={props.styles.mapLayersScroll}
            contentContainerStyle={props.styles.mapLayersScrollContent}>
            {panelSection(
              props.styles,
              mapLayersPanelSectionTitles[0],
              <>
                <BaseMapSection {...props} />
              </>,
            )}
            {panelSection(
              props.styles,
              mapLayersPanelSectionTitles[1],
              <>
                <View style={props.styles.mapLayersSwitchRow}>
                  <Text style={props.styles.mapLayersSwitchLabel}>
                    {mapLayersPanelSectionTitles[1]}
                  </Text>
                  <Switch
                    value={props.isTrafficEnabled}
                    onValueChange={props.onToggleTraffic}
                    disabled={trafficReasonText !== null}
                    accessibilityRole="switch"
                    accessibilityLabel={mapLayersPanelSectionTitles[1]}
                    accessibilityState={{
                      checked: props.isTrafficEnabled,
                      disabled: trafficReasonText !== null,
                    }}
                    testID={mapLayerTrafficIdentifier}
                  />
                </View>
                {trafficReasonText !== null ? (
                  <Text
                    style={props.styles.mapLayersReason}
                    testID="map-layer-traffic-reason">
                    {trafficReasonText}
                  </Text>
                ) : null}
              </>,
            )}
            {panelSection(
              props.styles,
              mapLayersPanelSectionTitles[2],
              <>
                {props.areSceneOverlaysAvailable ? null : (
                  <Text
                    style={props.styles.mapLayersReason}
                    testID="map-layer-scenes-reason">
                    {props.sceneOverlaysUnavailableReasonText ?? ''}
                  </Text>
                )}
                {props.sceneRows.map(sceneRow => (
                  <View
                    key={sceneRow.sceneKey}
                    style={props.styles.mapLayersSwitchRow}>
                    <Text style={props.styles.mapLayersSwitchLabel}>
                      {sceneRow.label}
                    </Text>
                    <Switch
                      value={sceneRow.isEnabled}
                      onValueChange={isEnabled =>
                        props.onToggleScene(sceneRow.sceneKey, isEnabled)
                      }
                      disabled={!props.areSceneOverlaysAvailable}
                      accessibilityRole="switch"
                      accessibilityLabel={sceneRow.label}
                      accessibilityState={{
                        checked: sceneRow.isEnabled,
                        disabled: !props.areSceneOverlaysAvailable,
                      }}
                      testID={sceneRow.identifier}
                    />
                  </View>
                ))}
                <Pressable
                  style={props.styles.mapLayersClearAction}
                  onPress={props.onClearScenes}
                  accessibilityRole="button"
                  testID={mapLayerClearOverlaysIdentifier}>
                  <Text style={props.styles.mapLayersClearActionText}>
                    {appStrings.demoScenesClearText}
                  </Text>
                </Pressable>
              </>,
            )}
            {panelSection(
              props.styles,
              mapLayersPanelSectionTitles[3],
              <>
                <View style={props.styles.mapLayersSwitchRow}>
                  <Text
                    style={props.styles.mapLayersSwitchLabel}
                    testID={deviceLocationStatusIdentifier}>
                    {deviceLocationPanelStatusText(props.locationStatus)}
                  </Text>
                  {isDeviceLocationSourceRunning(props.locationStatus) ? (
                    <Pressable
                      style={props.styles.mapLayersCloseButton}
                      onPress={props.onStopDeviceLocation}
                      accessibilityRole="button"
                      testID={deviceLocationStopIdentifier}>
                      <Text style={props.styles.mapLayersCloseText}>Stop</Text>
                    </Pressable>
                  ) : null}
                </View>
                {deviceLocationReasonText !== null ? (
                  <Text style={props.styles.mapLayersReason}>
                    {deviceLocationReasonText}
                  </Text>
                ) : null}
              </>,
            )}
            {props.statusLineText !== null ? (
              <Text
                style={props.styles.mapLayersStatus}
                testID={mapLayersStatusIdentifier}>
                {props.statusLineText}
              </Text>
            ) : null}
          </ScrollView>
        </View>
      </View>
    </Modal>
  );
}

/** The Base map section: raster descriptors first, then vector styles. */
function BaseMapSection(props: MapLayersPanelProps): React.JSX.Element {
  const rasterRows = props.baseRows.filter(
    row => row.selection.kind === 'raster',
  );
  const vectorRows = props.baseRows.filter(
    row => row.selection.kind === 'vector',
  );
  return (
    <>
      {rasterRows.map(row => (
        <BaseMapRow key={row.identifier} row={row} {...props} />
      ))}
      {vectorRows.length > 0 ? (
        <Text style={props.styles.mapLayersReason}>
          {mapLayersVectorStylesGroupTitle}
        </Text>
      ) : null}
      {vectorRows.map(row => (
        <BaseMapRow key={row.identifier} row={row} {...props} />
      ))}
    </>
  );
}

/** One base row: its name, its serving mark or refusal, and its act. */
function BaseMapRow(
  props: { row: MapLayerBaseRow } & MapLayersPanelProps,
): React.JSX.Element {
  const { row } = props;
  const isDisabled = row.disabledReasonText !== null;
  return (
    <Pressable
      style={[
        props.styles.mapLayersRow,
        row.isServing ? props.styles.mapLayersRowServing : null,
        isDisabled ? props.styles.mapLayersRowDisabled : null,
      ]}
      onPress={() => props.onSelectBaseRow(row)}
      disabled={isDisabled}
      accessibilityRole="button"
      accessibilityLabel={baseRowAccessibilityLabel(row)}
      accessibilityState={{ disabled: isDisabled, selected: row.isServing }}
      testID={row.identifier}>
      <Text style={props.styles.mapLayersRowName}>{row.displayName}</Text>
      <Text
        style={
          row.isServing
            ? props.styles.mapLayersRowStatusServing
            : props.styles.mapLayersRowStatus
        }>
        {baseRowStatusText(row)}
      </Text>
    </Pressable>
  );
}

/** One titled section of the panel. */
function panelSection(
  styles: ScreenStyles,
  title: string,
  content: ReactNode,
): React.JSX.Element {
  return (
    <View style={styles.mapLayersSection}>
      <Text style={styles.mapLayersSectionHeader}>{title}</Text>
      {content}
    </View>
  );
}
