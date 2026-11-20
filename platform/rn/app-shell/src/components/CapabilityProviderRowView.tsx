import React from 'react';
import { Pressable, Text, View } from 'react-native';

import {
  capabilityRegistryName,
  capabilityServingStatusText,
  type CapabilityProviderRow,
} from '../capabilityProviderState';
import type { buildScreenStyles } from '../screenStyles';
import { appStrings } from '../theme';

/**
 * One per-capability provider row (docs/Providers.md (Per-capability selection)):
 * the capability, the serving provider's name or the honest "Not configured"
 * line, and the registry's eligible providers as the picker. The two tile
 * rows are read-only because the map's layer control owns them.
 */
export function CapabilityProviderRowView(props: {
  row: CapabilityProviderRow;
  styles: ReturnType<typeof buildScreenStyles>;
  onSelect: (providerKey: string) => void;
}) {
  const capabilityName = capabilityRegistryName(props.row.contract);
  const servingChoice = props.row.choices.find(
    choice => choice.providerKey === props.row.servingProviderKey,
  );
  return (
    <View
      style={props.styles.settingsProviderRow}
      testID={'capability-provider-' + capabilityName}>
      <Text style={props.styles.settingsProviderName}>
        {props.row.capabilityDisplayName}
      </Text>
      <Text
        style={props.styles.settingsCapabilityServing}
        testID={'capability-provider-serving-' + capabilityName}>
        {capabilityServingStatusText(props.row)}
      </Text>
      {servingChoice !== undefined && servingChoice.demoBaseUrl !== null ? (
        <>
          <Text
            style={props.styles.settingsKeylessLine}
            testID={'capability-provider-endpoint-' + capabilityName}>
            {appStrings.settingsRoutingDemoEndpointFormat.replace(
              '{url}',
              servingChoice.demoBaseUrl,
            )}
          </Text>
          {servingChoice.demoBaseUrlNote !== null ? (
            <Text
              style={props.styles.settingsUnavailableText}
              testID={'capability-provider-endpoint-note-' + capabilityName}>
              {servingChoice.demoBaseUrlNote}
            </Text>
          ) : null}
        </>
      ) : null}
      {props.row.isReadOnly ? (
        <Text
          style={props.styles.settingsKeylessLine}
          testID={'capability-provider-readonly-' + capabilityName}>
          {props.row.readOnlyNoteText}
        </Text>
      ) : (
        <View style={props.styles.chipRow}>
          {props.row.choices.map(choice => {
            const isServing = choice.providerKey === props.row.servingProviderKey;
            return (
              <Pressable
                key={choice.providerKey}
                style={[props.styles.chip, isServing ? props.styles.chipActive : null]}
                onPress={() => props.onSelect(choice.providerKey)}
                testID={
                  'capability-provider-choice-' +
                  capabilityName +
                  '-' +
                  choice.providerKey
                }>
                <Text
                  style={[
                    props.styles.chipText,
                    isServing ? props.styles.chipTextActive : null,
                  ]}>
                  {choice.rowLabel}
                </Text>
              </Pressable>
            );
          })}
        </View>
      )}
      {props.row.detailText !== null ? (
        <Text
          style={props.styles.settingsUnavailableText}
          testID={'capability-provider-detail-' + capabilityName}>
          {props.row.detailText}
        </Text>
      ) : null}
    </View>
  );
}
