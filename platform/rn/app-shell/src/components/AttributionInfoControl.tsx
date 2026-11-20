/**
 * The attribution info control (§1/§4.2): the compact circular "ⓘ" at the
 * map's bottom-end corner and the panel carrying the serving base map's
 * attribution lines and terms notes.
 */

import React from 'react';
import { Modal, Pressable, Text } from 'react-native';

import type { buildScreenStyles } from '../screenStyles';
import { appStrings } from '../theme';

export function AttributionInfoControl(props: {
  isPanelOpen: boolean;
  onTogglePanel: () => void;
  onClosePanel: () => void;
  /** The serving base map's attribution line (empty claims nothing). */
  attributionText: string;
  /** The serving base map's terms note (empty renders no second block). */
  attributionTermsNotes: string[];
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  return (
    <>
      <Pressable
        style={[
          props.styles.attributionInfoControl,
          props.isPanelOpen && props.styles.attributionInfoControlActive,
        ]}
        onPress={props.onTogglePanel}
        accessibilityRole="button"
        accessibilityLabel={appStrings.attributionInfoAccessibilityText}
        accessibilityState={{ expanded: props.isPanelOpen }}
        testID="attribution-info-control">
        <Text style={props.styles.attributionInfoGlyph}>
          {appStrings.attributionInfoSymbol}
        </Text>
      </Pressable>
      <Modal
        visible={props.isPanelOpen}
        transparent
        animationType="fade"
        onRequestClose={props.onClosePanel}>
        <Pressable
          style={props.styles.attributionPanelBackdrop}
          onPress={props.onClosePanel}
          testID="attribution-panel-backdrop">
          <Pressable
            style={props.styles.attributionPanel}
            onPress={() => {}}
            testID="attribution-panel">
            <Text style={props.styles.attributionPanelTitle}>
              {appStrings.attributionPanelTitleText}
            </Text>
            <Text style={props.styles.attribution} selectable testID="attribution">
              {props.attributionText}
            </Text>
            {props.attributionTermsNotes.length > 0 ? (
              <Text style={props.styles.attributionTermsNote} selectable>
                {props.attributionTermsNotes.join('\n\n')}
              </Text>
            ) : null}
          </Pressable>
        </Pressable>
      </Modal>
    </>
  );
}
