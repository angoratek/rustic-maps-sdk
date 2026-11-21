/**
 * The plan search view and the field and Directions controls it composes
 * (§4.1).
 */

import React, { useRef } from 'react';
import { Pressable, Text, TextInput, View } from 'react-native';

import {
  RouteStrategy,
  TransportationMode,
} from '@angoratek/rustic-maps-sdk/src/generated/rustic_maps.ts';
import type { PlaceRow } from '../RefappNative';
import type {
  DirectionsControlState,
  DirectionsRouteStrategyName,
  DirectionsTransportationModeName,
} from '../directionsSelection';
import {
  myLocationChipFilledMark,
  planDestinationFieldMarker,
  planOriginFieldMarker,
  type PlanSearchField,
} from '../helpers/planFields';
import type {
  AutocompleteProviderSelection,
  PlanAutocompleteGateway,
} from '../planSearchAutocomplete';
import { usePlanAutocomplete } from '../planSearchAutocomplete';
import type { buildScreenStyles } from '../screenStyles';
import { appStrings } from '../theme';

/**
 * The plan sheet's Directions control (PRD.md §6 item 36):
 * the Car/Walk mode choice, then the Fastest/Shortest strategy for car plans
 * only. A choice the serving routing provider cannot honour is disabled and
 * states its reason.
 */
function DirectionsControls(props: {
  control: DirectionsControlState;
  styles: ReturnType<typeof buildScreenStyles>;
  onSelectTransportationMode: (
    transportationModeName: DirectionsTransportationModeName,
  ) => void;
  onSelectRouteStrategy: (
    routeStrategyName: DirectionsRouteStrategyName,
  ) => void;
}) {
  const isCarSelected =
    props.control.selection.transportationMode === TransportationMode.Automobile;
  const isWalkSelected =
    props.control.selection.transportationMode === TransportationMode.Pedestrian;
  const isFastestSelected =
    props.control.selection.routeStrategy === RouteStrategy.Fastest;
  const isShortestSelected =
    props.control.selection.routeStrategy === RouteStrategy.Shortest;
  const modeReasonText = !props.control.car.isEnabled
    ? props.control.car.reasonText
    : !props.control.walk.isEnabled
      ? props.control.walk.reasonText
      : '';
  return (
    <View testID="directions-controls">
      <Text style={props.styles.sheetHeader}>
        {appStrings.directionsTitleText}
      </Text>
      <Text style={props.styles.searchResultsHeader}>
        {appStrings.directionsModeRowTitleText}
      </Text>
      <View style={props.styles.chipRow} testID="directions-mode-row">
        <Pressable
          style={[
            props.styles.chip,
            isCarSelected ? props.styles.chipActive : null,
            !props.control.car.isEnabled ? props.styles.actionControlDisabled : null,
          ]}
          disabled={!props.control.car.isEnabled}
          accessibilityState={{ selected: isCarSelected }}
          onPress={() => props.onSelectTransportationMode('automobile')}
          testID="directions-mode-car">
          <Text
            style={[
              props.styles.chipText,
              isCarSelected ? props.styles.chipTextActive : null,
            ]}>
            {appStrings.directionsModeCarText}
          </Text>
        </Pressable>
        <Pressable
          style={[
            props.styles.chip,
            isWalkSelected ? props.styles.chipActive : null,
            !props.control.walk.isEnabled ? props.styles.actionControlDisabled : null,
          ]}
          disabled={!props.control.walk.isEnabled}
          accessibilityState={{ selected: isWalkSelected }}
          onPress={() => props.onSelectTransportationMode('pedestrian')}
          testID="directions-mode-walk">
          <Text
            style={[
              props.styles.chipText,
              isWalkSelected ? props.styles.chipTextActive : null,
            ]}>
            {appStrings.directionsModeWalkText}
          </Text>
        </Pressable>
      </View>
      {modeReasonText !== '' ? (
        <Text
          style={props.styles.reasonStripText}
          testID="directions-mode-reason">
          {modeReasonText}
        </Text>
      ) : null}
      {props.control.isStrategyRowVisible ? (
        <>
          <Text style={props.styles.searchResultsHeader}>
            {appStrings.directionsStrategyRowTitleText}
          </Text>
          <View style={props.styles.chipRow} testID="directions-strategy-row">
            <Pressable
              style={[
                props.styles.chip,
                isFastestSelected ? props.styles.chipActive : null,
              ]}
              accessibilityState={{ selected: isFastestSelected }}
              onPress={() => props.onSelectRouteStrategy('fastest')}
              testID="directions-strategy-fastest">
              <Text
                style={[
                  props.styles.chipText,
                  isFastestSelected ? props.styles.chipTextActive : null,
                ]}>
                {appStrings.directionsStrategyFastestText}
              </Text>
            </Pressable>
            <Pressable
              style={[
                props.styles.chip,
                isShortestSelected ? props.styles.chipActive : null,
                !props.control.shortest.isEnabled
                  ? props.styles.actionControlDisabled
                  : null,
              ]}
              disabled={!props.control.shortest.isEnabled}
              accessibilityState={{ selected: isShortestSelected }}
              onPress={() => props.onSelectRouteStrategy('shortest')}
              testID="directions-strategy-shortest">
              <Text
                style={[
                  props.styles.chipText,
                  isShortestSelected ? props.styles.chipTextActive : null,
                ]}>
                {appStrings.directionsStrategyShortestText}
              </Text>
            </Pressable>
          </View>
          {!props.control.shortest.isEnabled &&
          props.control.shortest.reasonText !== modeReasonText ? (
            <Text
              style={props.styles.reasonStripText}
              testID="directions-strategy-reason">
              {props.control.shortest.reasonText}
            </Text>
          ) : null}
        </>
      ) : null}
    </View>
  );
}

/**
 * One labeled plan field (§4.1): its A/B marker and label, the input with its
 * inline Search action, the type-ahead list the field owns, and its My
 * location chip.
 */
function PlanFieldSearchControl(props: {
  planSearchField: PlanSearchField;
  marker: string;
  labelText: string;
  hintText: string;
  fieldText: string;
  searchFieldTestID: string;
  searchActionTestID: string;
  myLocationTestID: string;
  /** The reason the My location chip cannot start, or null while it works. */
  myLocationDisabledReason: string | null;
  /** Whether this field is the one the My location chip filled. */
  isMyLocationFilled: boolean;
  styles: ReturnType<typeof buildScreenStyles>;
  autocompleteProviderSelection: AutocompleteProviderSelection | null;
  autocompleteGateway: PlanAutocompleteGateway | null;
  onChangeText: (text: string) => void;
  onSearch: (planSearchField: PlanSearchField, queryText: string) => void;
  onFieldFocused: (planSearchField: PlanSearchField) => void;
  onFillMyLocation: (planSearchField: PlanSearchField) => void;
  onSelectSuggestion: (
    planSearchField: PlanSearchField,
    placeRow: PlaceRow,
  ) => void;
}) {
  // Only text the visitor typed asks a provider: a programmatic fill (a
  // result assignment, the My location chip, a swap) reaches the field
  // through the value prop and never through this handler.
  const userEditedQueryRef = useRef<string | null>(null);
  const autocomplete = usePlanAutocomplete({
    queryText: props.fieldText,
    userEditedQueryText: userEditedQueryRef.current,
    providerSelection: props.autocompleteProviderSelection,
    gateway: props.autocompleteGateway,
    onSelectSuggestion: placeRow =>
      props.onSelectSuggestion(props.planSearchField, placeRow),
  });
  return (
    <View style={props.styles.searchFieldGroup}>
      <View style={props.styles.searchFieldLabelRow}>
        <Text
          style={props.styles.searchFieldMarker}
          testID={`${props.planSearchField}-field-marker`}>
          {props.marker}
        </Text>
        <Text style={props.styles.searchFieldLabel}>{props.labelText}</Text>
      </View>
      <View style={props.styles.searchFieldControlRow}>
        <TextInput
          style={[
            props.styles.searchFieldInput,
            props.styles.searchFieldInputFlexible,
          ]}
          value={props.fieldText}
          onChangeText={text => {
            userEditedQueryRef.current = text;
            props.onChangeText(text);
          }}
          onFocus={() => props.onFieldFocused(props.planSearchField)}
          onSubmitEditing={() =>
            props.onSearch(props.planSearchField, props.fieldText)
          }
          placeholder={props.hintText}
          returnKeyType="search"
          testID={props.searchFieldTestID}
        />
        <Pressable
          style={props.styles.searchFieldAction}
          onPress={() => props.onSearch(props.planSearchField, props.fieldText)}
          testID={props.searchActionTestID}>
          <Text style={props.styles.searchFieldActionText}>
            {appStrings.planSearchActionText}
          </Text>
        </Pressable>
      </View>
      {autocomplete.status === 'failed' ? (
        <Text
          style={props.styles.searchResultsHeader}
          testID={`${props.planSearchField}-autocomplete-error`}>
          {autocomplete.errorText}
        </Text>
      ) : null}
      {autocomplete.status === 'noMatches' ? (
        <View
          style={props.styles.resultList}
          testID={`${props.planSearchField}-autocomplete-list`}>
          <View
            style={props.styles.resultRow}
            testID={`${props.planSearchField}-autocomplete-no-matches`}>
            <Text style={props.styles.resultRowSecondary}>
              {appStrings.planAutocompleteNoMatchesText}
            </Text>
          </View>
        </View>
      ) : null}
      {autocomplete.suggestions.length > 0 ? (
        <View
          style={props.styles.resultList}
          testID={`${props.planSearchField}-autocomplete-list`}>
          {autocomplete.suggestions.map((suggestion, suggestionIndex) => (
            <Pressable
              key={suggestion.structuredAddressText}
              style={props.styles.resultRow}
              accessibilityLabel={suggestion.structuredAddressText}
              onPress={() => autocomplete.selectSuggestion(suggestion)}
              testID={`${props.planSearchField}-autocomplete-row-${suggestionIndex}`}>
              <Text style={props.styles.resultRowName}>
                {suggestion.structuredAddressText}
              </Text>
            </Pressable>
          ))}
        </View>
      ) : null}
      <Pressable
        style={[
          props.styles.myLocationChip,
          props.isMyLocationFilled ? props.styles.myLocationChipFilled : null,
          props.myLocationDisabledReason !== null
            ? props.styles.myLocationChipDisabled
            : null,
        ]}
        disabled={props.myLocationDisabledReason !== null}
        accessibilityState={{
          disabled: props.myLocationDisabledReason !== null,
          selected: props.isMyLocationFilled,
        }}
        onPress={() => props.onFillMyLocation(props.planSearchField)}
        accessibilityLabel={`Use my location as the ${props.planSearchField}`}
        testID={props.myLocationTestID}>
        <Text style={props.styles.myLocationChipText}>
          {props.isMyLocationFilled
            ? `${appStrings.planMyLocationActionText} ${myLocationChipFilledMark}`
            : appStrings.planMyLocationActionText}
        </Text>
      </Pressable>
      {props.myLocationDisabledReason !== null ? (
        <Text
          style={props.styles.searchResultsHeader}
          testID={`${props.myLocationTestID}-reason`}>
          {props.myLocationDisabledReason}
        </Text>
      ) : null}
    </View>
  );
}

/**
 * The plan search view (§4.1 and docs/ReferenceAppUX.md):
 * two visibly labeled fields (A/B markers), each with its inline Search
 * action and My location chip, one swap action, the Directions choices, the
 * shared result rows with the assigned-row state, and Done back to the half
 * sheet.
 */
export function PlanSearchView(props: {
  originSearchText: string;
  destinationSearchText: string;
  searchRows: PlaceRow[];
  /** Whether the rows are the shared artifact's demo places (§4.1's keyless search). */
  searchRowsAreDemoPlaces: boolean;
  /** The keyless search's honest no-match line, null while a match stands. */
  demoSearchNoMatchText: string | null;
  searchInFlight: boolean;
  assignedOriginText: string | null;
  assignedDestinationText: string | null;
  planRouteReasonText: string;
  canPlanRoute: boolean;
  directionsControl: DirectionsControlState;
  styles: ReturnType<typeof buildScreenStyles>;
  /** The reason each My location chip cannot start, or null while it works. */
  myLocationDisabledReason: string | null;
  /** Which fields a My location chip filled this session. */
  myLocationFilledFields: { origin: boolean; destination: boolean };
  /** The credential state's type-ahead provider, or null when none is usable. */
  autocompleteProviderSelection: AutocompleteProviderSelection | null;
  /** The SDK seam the fields' type-ahead asks. */
  autocompleteGateway: PlanAutocompleteGateway | null;
  onOriginSearchTextChange: (text: string) => void;
  onDestinationSearchTextChange: (text: string) => void;
  onSearch: (planSearchField: PlanSearchField, queryText: string) => void;
  onSelectAutocompleteSuggestion: (
    planSearchField: PlanSearchField,
    placeRow: PlaceRow,
  ) => void;
  onAssign: (placeRow: PlaceRow) => void;
  onPlanRoute: () => void;
  onSelectDirectionsTransportationMode: (
    transportationModeName: DirectionsTransportationModeName,
  ) => void;
  onSelectDirectionsRouteStrategy: (
    routeStrategyName: DirectionsRouteStrategyName,
  ) => void;
  onFieldFocused: (planSearchField: PlanSearchField) => void;
  onFillMyLocation: (planSearchField: PlanSearchField) => void;
  onSwapFields: () => void;
  onDone: () => void;
}) {
  return (
    <>
      <Text style={props.styles.searchViewHeading} testID="search-heading">
        {appStrings.searchHeadingText}
      </Text>
      <PlanFieldSearchControl
        planSearchField="origin"
        marker={planOriginFieldMarker}
        labelText={appStrings.planOriginLabelText}
        hintText={appStrings.originFieldHintText}
        fieldText={props.originSearchText}
        onChangeText={props.onOriginSearchTextChange}
        searchFieldTestID="origin-search-field"
        searchActionTestID="origin-search-action"
        myLocationTestID="origin-my-location"
        myLocationDisabledReason={props.myLocationDisabledReason}
        isMyLocationFilled={props.myLocationFilledFields.origin}
        styles={props.styles}
        autocompleteProviderSelection={props.autocompleteProviderSelection}
        autocompleteGateway={props.autocompleteGateway}
        onSearch={props.onSearch}
        onFieldFocused={props.onFieldFocused}
        onFillMyLocation={props.onFillMyLocation}
        onSelectSuggestion={props.onSelectAutocompleteSuggestion}
      />
      <PlanFieldSearchControl
        planSearchField="destination"
        marker={planDestinationFieldMarker}
        labelText={appStrings.planDestinationLabelText}
        hintText={appStrings.destinationFieldHintText}
        fieldText={props.destinationSearchText}
        onChangeText={props.onDestinationSearchTextChange}
        searchFieldTestID="destination-search-field"
        searchActionTestID="destination-search-action"
        myLocationTestID="destination-my-location"
        myLocationDisabledReason={props.myLocationDisabledReason}
        isMyLocationFilled={props.myLocationFilledFields.destination}
        styles={props.styles}
        autocompleteProviderSelection={props.autocompleteProviderSelection}
        autocompleteGateway={props.autocompleteGateway}
        onSearch={props.onSearch}
        onFieldFocused={props.onFieldFocused}
        onFillMyLocation={props.onFillMyLocation}
        onSelectSuggestion={props.onSelectAutocompleteSuggestion}
      />
      <View style={props.styles.peekActionRow}>
        <Pressable
          style={props.styles.actionControl}
          onPress={props.onSwapFields}
          testID="swap-plan-fields">
          <Text style={props.styles.actionControlText}>
            {appStrings.planSwapActionText}
          </Text>
        </Pressable>
        <Pressable style={props.styles.actionControl} onPress={props.onDone} testID="search-done">
          <Text style={props.styles.actionControlText}>{appStrings.searchDoneText}</Text>
        </Pressable>
      </View>
      {props.planRouteReasonText !== '' ? (
        <Text style={props.styles.searchResultsHeader} testID="plan-route-reason">
          {props.planRouteReasonText}
        </Text>
      ) : null}
      <DirectionsControls
        control={props.directionsControl}
        styles={props.styles}
        onSelectTransportationMode={props.onSelectDirectionsTransportationMode}
        onSelectRouteStrategy={props.onSelectDirectionsRouteStrategy}
      />
      <Pressable
        style={[
          props.styles.actionControl,
          props.styles.actionControlPrimary,
          !props.canPlanRoute ? props.styles.actionControlDisabled : null,
        ]}
        disabled={!props.canPlanRoute}
        onPress={props.onPlanRoute}
        testID="plan-route">
        <Text style={[props.styles.actionControlText, props.styles.actionControlTextPrimary]}>
          {appStrings.planRouteButtonText}
        </Text>
      </Pressable>
      <Text style={props.styles.searchResultsHeader} testID="search-results-header">
        {props.searchInFlight
          ? appStrings.searchInFlightText
          : props.demoSearchNoMatchText !== null
            ? props.demoSearchNoMatchText
            : props.searchRows.length > 0
              ? props.searchRowsAreDemoPlaces
                ? appStrings.demoPlacesHeadingText
                : appStrings.searchResultsLabelText
              : props.destinationSearchText === '' && props.originSearchText === ''
                ? appStrings.searchHintText
                : ''}
      </Text>
      <View style={props.styles.resultList} testID="search-results-list">
        {props.searchRows.map(placeRow => {
          const assignedToOrigin = props.assignedOriginText === placeRow.structuredAddressText;
          const assignedToDestination =
            props.assignedDestinationText === placeRow.structuredAddressText;
          return (
            <Pressable
              key={placeRow.structuredAddressText}
              style={[
                props.styles.resultRow,
                assignedToOrigin || assignedToDestination ? props.styles.resultRowAssigned : null,
              ]}
              onPress={() => props.onAssign(placeRow)}
              testID={'search-result-' + placeRow.structuredAddressText}>
              <Text style={props.styles.resultRowName}>{placeRow.structuredAddressText}</Text>
              <Text style={props.styles.resultRowSecondary}>
                {(assignedToOrigin ? appStrings.planOriginLabelText + ' · ' : '') +
                  (assignedToDestination ? appStrings.planDestinationLabelText + ' · ' : '') +
                  (placeRow.noteText ?? placeRow.qualityText)}
              </Text>
            </Pressable>
          );
        })}
      </View>
    </>
  );
}
