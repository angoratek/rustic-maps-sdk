import React from 'react';
import { PixelRatio, Text, View } from 'react-native';

import { distanceLabelText, selectScaleBarScale } from '../mapControls';
import type { buildScreenStyles } from '../screenStyles';

/**
 * The scale bar (§4.2): bottom-start, built on the camera's
 * ground-meters-per-screen-pixel answer with the §2.4 width range and the
 * 1-2-3-5-10 distance ladder; no answer renders no bar.
 */
export function ScaleBarOverlay(props: {
  groundMetersPerScreenPixel: number | undefined;
  styles: ReturnType<typeof buildScreenStyles>;
}) {
  if (props.groundMetersPerScreenPixel === undefined) {
    return null;
  }
  const selectedScale = selectScaleBarScale(props.groundMetersPerScreenPixel);
  if (selectedScale === null) {
    return null;
  }
  // The selected width is in physical viewport pixels; the drawing unit is
  // the density-independent point (the Compose scale bar's same conversion).
  const barWidthPoints = selectedScale.widthPixels / PixelRatio.get();
  return (
    <View style={props.styles.scaleBar} testID="scale-bar">
      <Text style={props.styles.scaleBarText}>
        {distanceLabelText(selectedScale.distanceMeters)}
      </Text>
      <View style={[props.styles.scaleBarLine, { width: barWidthPoints }]} />
    </View>
  );
}
