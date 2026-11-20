import React from 'react';
import { Pressable, Switch, Text, TextInput, View } from 'react-native';

import {
  canApplyProviderCredential,
  isProviderCredentialRegistrationAvailable,
  providerCredentialStatusText,
  type ProviderCredentialRowState,
} from '../providerCredentials';
import type { ProviderCredentialDescriptor } from '../providerRegistry';
import type { buildScreenStyles } from '../screenStyles';
import { appStrings } from '../theme';

/**
 * One provider-credential row (§3): the descriptor's labels, the masked
 * secure entry, Apply/Clear, the honest status line, and the opt-in
 * "Remember on this device" toggle (default off). A provider the SDK does not
 * register on React Native renders disabled-with-reason instead of pretending
 * to work.
 */
export function ProviderCredentialRow(props: {
  descriptor: ProviderCredentialDescriptor;
  rowState: ProviderCredentialRowState;
  /** The hint line: the descriptor's own, or the base map's "enter a key" pointer. */
  hintText: string;
  styles: ReturnType<typeof buildScreenStyles>;
  onChangeCredential: (credential: string) => void;
  onToggleRemember: (isRememberEnabled: boolean) => void;
  onApply: () => void;
  onClear: () => void;
}) {
  const isRegistrationAvailable = isProviderCredentialRegistrationAvailable(
    props.descriptor,
  );
  const isApplyEnabled =
    isRegistrationAvailable && canApplyProviderCredential(props.rowState);
  return (
    <View
      style={props.styles.settingsProviderRow}
      testID={'provider-credential-row-' + props.descriptor.provider_key}>
      <Text style={props.styles.settingsProviderName}>
        {props.descriptor.display_name}
      </Text>
      <Text style={props.styles.settingsCredentialLabel}>
        {props.descriptor.credential_label}
      </Text>
      <TextInput
        style={props.styles.settingsCredentialInput}
        value={props.rowState.enteredCredential}
        onChangeText={props.onChangeCredential}
        secureTextEntry
        autoCapitalize="none"
        autoCorrect={false}
        placeholder={props.descriptor.credential_label}
        editable={isRegistrationAvailable}
        testID={'provider-credential-input-' + props.descriptor.provider_key}
      />
      <Text
        style={props.styles.settingsCredentialStatus}
        testID={'provider-credential-status-' + props.descriptor.provider_key}>
        {providerCredentialStatusText(props.descriptor.display_name, props.rowState)}
      </Text>
      <View style={props.styles.settingsRememberRow}>
        <Text style={props.styles.settingsRememberLabel}>
          {appStrings.providerCredentialRememberLabelText}
        </Text>
        <Switch
          value={props.rowState.isRememberEnabled}
          onValueChange={props.onToggleRemember}
          disabled={!isRegistrationAvailable}
          testID={'provider-credential-remember-' + props.descriptor.provider_key}
        />
      </View>
      <View style={props.styles.peekActionRow}>
        <Pressable
          style={[
            props.styles.actionControl,
            props.styles.actionControlPrimary,
            isApplyEnabled ? null : props.styles.actionControlDisabled,
          ]}
          onPress={props.onApply}
          disabled={!isApplyEnabled}
          testID={'provider-credential-apply-' + props.descriptor.provider_key}>
          <Text style={props.styles.actionControlText}>
            {appStrings.providerCredentialApplyButtonText}
          </Text>
        </Pressable>
        <Pressable
          style={props.styles.actionControl}
          onPress={props.onClear}
          disabled={!isRegistrationAvailable}
          testID={'provider-credential-clear-' + props.descriptor.provider_key}>
          <Text style={props.styles.actionControlText}>
            {appStrings.providerCredentialClearButtonText}
          </Text>
        </Pressable>
      </View>
      {isRegistrationAvailable ? (
        <>
          <Text style={props.styles.settingsCredentialHint}>
            {props.hintText}
          </Text>
          <Text style={props.styles.settingsCredentialAttribution}>
            {props.descriptor.attribution_text}
          </Text>
        </>
      ) : (
        <Text
          style={props.styles.settingsUnavailableText}
          testID={'provider-credential-unavailable-' + props.descriptor.provider_key}>
          {appStrings.providerCredentialUnavailableReasonText}
        </Text>
      )}
    </View>
  );
}
