//! Parsing and validation of the JavaScript descriptor objects the canvas
//! verbs accept.

use super::*;

/// Reads one JavaScript object field, mapping a getter failure (never seen
/// for a plain object) to a detail message.
pub(super) fn javascript_field(object: &Object, field_name: &str) -> Result<JsValue, String> {
    Reflect::get(object, &JsValue::from_str(field_name))
        .map_err(|error| format!("the field {field_name} could not be read: {error:?}"))
}

/// Reads one optional finite JavaScript number field; an absent (`undefined`
/// or `null`) field is `None`, anything unparsable or non-finite an error.
pub(super) fn optional_javascript_number_field(
    overlay_object: &Object,
    field_name: &str,
) -> Result<Option<f64>, String> {
    let field_value = javascript_field(overlay_object, field_name)?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(None);
    }
    let field_number = field_value
        .as_f64()
        .ok_or_else(|| format!("the field {field_name} must be a number"))?;
    if !field_number.is_finite() {
        return Err(format!("the field {field_name} must be finite"));
    }
    Ok(Some(field_number))
}

/// Reads one optional pacing-rate field. An absent (`undefined` or `null`)
/// field is `None` and a present non-number is refused; the numeric range is
/// left to the provider's pacing validation, whose refusal names the source
/// the rate belongs to.
pub(super) fn optional_javascript_pacing_rate_field(
    source_object: &Object,
    field_name: &str,
) -> Result<Option<f64>, JsValue> {
    let field_value = javascript_field(source_object, field_name).map_err(|failure_message| {
        failure_with_code(ERROR_CODE_INVALID_ARGUMENT, &failure_message)
    })?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(None);
    }
    field_value.as_f64().map(Some).ok_or_else(|| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!("the vector base source {field_name} must be a number"),
        )
    })
}

/// Reads one required JavaScript number field, mapping a getter failure
/// (never seen for a plain object) to a detail message.
pub(super) fn required_javascript_number_field(
    overlay_object: &Object,
    field_name: &str,
) -> Result<f64, String> {
    optional_javascript_number_field(overlay_object, field_name)?
        .ok_or_else(|| format!("the field {field_name} is missing"))
}

/// Reads one required JavaScript string field.
pub(super) fn required_javascript_string_field(
    overlay_object: &Object,
    field_name: &str,
) -> Result<String, String> {
    javascript_field(overlay_object, field_name)?
        .as_string()
        .ok_or_else(|| format!("the field {field_name} must be a string"))
}

/// Normalizes one JavaScript value into the host-testable field shape the
/// vector-source declaration validation reads. Arrays and plain objects
/// recurse, so a malformed nested entry is refused by the validator instead of
/// being silently skipped here.
pub(super) fn source_field_value_from_javascript(field_value: &JsValue) -> SourceFieldValue {
    if field_value.is_undefined() || field_value.is_null() {
        return SourceFieldValue::Absent;
    }
    if let Some(text) = field_value.as_string() {
        return SourceFieldValue::Text(text);
    }
    if let Some(list_array) = field_value.dyn_ref::<Array>() {
        let mut entries = Vec::with_capacity(list_array.length() as usize);
        for entry_index in 0..list_array.length() {
            entries.push(source_field_value_from_javascript(
                &list_array.get(entry_index),
            ));
        }
        return SourceFieldValue::List(entries);
    }
    if let Some(object) = field_value.dyn_ref::<Object>() {
        let object_names = Object::keys(object);
        let mut named_fields = Vec::with_capacity(object_names.length() as usize);
        for name_index in 0..object_names.length() {
            let name_value = object_names.get(name_index);
            if let Some(field_name) = name_value.as_string()
                && let Ok(named_field_value) = Reflect::get(object, &name_value)
            {
                named_fields.push((
                    field_name,
                    source_field_value_from_javascript(&named_field_value),
                ));
            }
        }
        return SourceFieldValue::Object(named_fields);
    }
    SourceFieldValue::Other
}

/// The provider key and style label a provider-backed vector base source
/// names, read from the plain JavaScript object shape.
pub(super) fn provider_vector_source_fields(source: &JsValue) -> Result<(String, String), JsValue> {
    let source_object: &Object = source.dyn_ref().ok_or_else(|| {
        failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            "the provider vector base source must be a plain JavaScript object",
        )
    })?;
    let provider_key =
        required_javascript_string_field(source_object, PROVIDER_VECTOR_SOURCE_FIELD_PROVIDER_KEY)
            .map_err(|failure_message| {
                failure_with_code(ERROR_CODE_INVALID_ARGUMENT, &failure_message)
            })?;
    let style_id =
        required_javascript_string_field(source_object, PROVIDER_VECTOR_SOURCE_FIELD_STYLE_ID)
            .map_err(|failure_message| {
                failure_with_code(ERROR_CODE_INVALID_ARGUMENT, &failure_message)
            })?;
    Ok((provider_key, style_id))
}

/// Reads one required JavaScript zoom-level field as the contract's `u8`: an
/// integer inside the web-mercator grid's representable range. A non-integer
/// or out-of-range value names the field in the refusal.
pub(super) fn javascript_zoom_level_field(
    descriptor_object: &Object,
    field_name: &str,
) -> Result<u8, JsValue> {
    let zoom_level = required_javascript_number_field(descriptor_object, field_name).map_err(
        |failure_message| failure_with_code(ERROR_CODE_INVALID_ARGUMENT, &failure_message),
    )?;
    if zoom_level.fract() != 0.0
        || !(0.0..=f64::from(MAXIMUM_TILE_GRID_ZOOM_LEVEL)).contains(&zoom_level)
    {
        return Err(failure_with_code(
            ERROR_CODE_INVALID_ARGUMENT,
            &format!(
                "the field {field_name} must be a whole zoom level between 0 and \
                 {MAXIMUM_TILE_GRID_ZOOM_LEVEL}"
            ),
        ));
    }
    Ok(zoom_level as u8)
}

/// Validates one plain JavaScript coordinate object into its native
/// geographic coordinate (the shared `from_degrees` validation reports the
/// underlying range problem).
pub(super) fn validated_javascript_coordinate(
    coordinate_value: &JsValue,
) -> Result<GeographicCoordinate, String> {
    let coordinate_object: &Object = coordinate_value.dyn_ref().ok_or_else(|| {
        format!(
            "each coordinate must be a plain object with \
             {OVERLAY_FIELD_LATITUDE} and {OVERLAY_FIELD_LONGITUDE} numbers"
        )
    })?;
    let latitude = required_javascript_number_field(coordinate_object, OVERLAY_FIELD_LATITUDE)?;
    let longitude = required_javascript_number_field(coordinate_object, OVERLAY_FIELD_LONGITUDE)?;
    validated_coordinate(latitude, longitude)
        .map_err(|sdk_error| format!("invalid coordinate: {sdk_error}"))
}

/// Validates a JavaScript array of plain coordinate objects into one native
/// coordinate list, naming the offending entry in the error.
pub(super) fn validated_javascript_coordinate_list(
    list_value: &JsValue,
    list_description: &str,
) -> Result<Vec<GeographicCoordinate>, String> {
    let list_array: &Array = list_value
        .dyn_ref()
        .ok_or_else(|| format!("{list_description} must be an array of coordinate objects"))?;
    let mut coordinates = Vec::with_capacity(list_array.length() as usize);
    for coordinate_index in 0..list_array.length() {
        coordinates.push(
            validated_javascript_coordinate(&list_array.get(coordinate_index)).map_err(
                |message| {
                    format!(
                        "{list_description} coordinate {coordinate_index} is invalid: {message}"
                    )
                },
            )?,
        );
    }
    Ok(coordinates)
}

/// Validates an optional JavaScript color override into its native RGBA
/// (channels in 0.0..=1.0, every channel required when the override appears).
pub(super) fn optional_javascript_color(
    overlay_object: &Object,
) -> Result<Option<[f32; 4]>, String> {
    optional_javascript_color_field(overlay_object, OVERLAY_FIELD_COLOR)
}

/// Validates an optional JavaScript color object under the given field name
/// into its native RGBA (channels in 0.0..=1.0, every channel required when
/// the override appears); the shared color rules of the overlay overrides
/// and the location-indicator style.
pub(super) fn optional_javascript_color_field(
    descriptor_object: &Object,
    color_field_name: &str,
) -> Result<Option<[f32; 4]>, String> {
    let color_value = javascript_field(descriptor_object, color_field_name)?;
    if color_value.is_undefined() || color_value.is_null() {
        return Ok(None);
    }
    let color_object: &Object = color_value
        .dyn_ref()
        .ok_or_else(|| format!("the {color_field_name} override must be a plain object"))?;
    let mut rgba_color = [0.0_f32; 4];
    for (channel_name, channel_index) in COLOR_CHANNELS {
        let channel_value = required_javascript_number_field(color_object, channel_name)?;
        if !(0.0..=1.0).contains(&channel_value) {
            return Err(format!(
                "the color field {channel_name} must be within 0.0..=1.0"
            ));
        }
        rgba_color[channel_index] = channel_value as f32;
    }
    Ok(Some(rgba_color))
}

/// Channel-name/RGBA-slot pairs color overrides read, in field order.
pub(super) const COLOR_CHANNELS: [(&str, usize); 4] = [
    (COLOR_FIELD_RED, 0),
    (COLOR_FIELD_GREEN, 1),
    (COLOR_FIELD_BLUE, 2),
    (COLOR_FIELD_ALPHA, 3),
];

/// Rejects a non-positive screen-space width or radius override.
pub(super) fn validated_positive_screen_pixels(
    screen_pixels: f64,
    field_name: &str,
) -> Result<(), String> {
    if screen_pixels <= 0.0 {
        return Err(format!("the field {field_name} must be positive"));
    }
    Ok(())
}

/// Reads one optional JavaScript boolean field; an absent (`undefined` or
/// `null`) field is `None`, anything non-boolean an error.
pub(super) fn optional_javascript_boolean_field(
    descriptor_object: &Object,
    field_name: &str,
) -> Result<Option<bool>, String> {
    let field_value = javascript_field(descriptor_object, field_name)?;
    if field_value.is_undefined() || field_value.is_null() {
        return Ok(None);
    }
    field_value
        .as_bool()
        .map(Some)
        .ok_or_else(|| format!("the field {field_name} must be a boolean"))
}

/// Validates one plain JavaScript location-indicator state descriptor into
/// its native [`LocationIndicatorState`]: the coordinate is required and
/// range-validated, the optional scalars pass through verbatim (the native
/// indicator state's rule).
pub(super) fn validated_location_indicator_state(
    state_object: &Object,
) -> Result<LocationIndicatorState, String> {
    let coordinate_value = javascript_field(state_object, INDICATOR_STATE_FIELD_COORDINATE)?;
    let coordinate = validated_javascript_coordinate(&coordinate_value)?;
    let course_heading_degrees =
        optional_javascript_number_field(state_object, INDICATOR_FIELD_COURSE_HEADING_DEGREES)?;
    let horizontal_accuracy_meters =
        optional_javascript_number_field(state_object, INDICATOR_FIELD_HORIZONTAL_ACCURACY_METERS)?;
    Ok(LocationIndicatorState {
        coordinate,
        course_heading_degrees,
        horizontal_accuracy_meters,
    })
}

/// Validates one plain JavaScript location-indicator style descriptor into
/// its native [`LocationIndicatorStyle`]: the visual kind is required, the
/// style's optional overrides mirror the native record conversion's rules
/// (positive finite radius, channels in 0.0..=1.0) and keep the documented
/// rustic-maps-rendering defaults wherever the descriptor gives no override.
pub(super) fn validated_location_indicator_style(
    style_object: &Object,
) -> Result<LocationIndicatorStyle, String> {
    let visual_kind =
        match required_javascript_string_field(style_object, INDICATOR_FIELD_VISUAL_KIND)?.as_str()
        {
            INDICATOR_KIND_TRACKING_DOT => LocationIndicatorVisualKind::TrackingDot,
            INDICATOR_KIND_GUIDED_COURSE_ARROW => LocationIndicatorVisualKind::GuidedCourseArrow,
            unknown_kind => {
                return Err(format!(
                    "unknown location-indicator visual kind {unknown_kind:?} (expected one of \
                 {INDICATOR_KIND_TRACKING_DOT:?}, {INDICATOR_KIND_GUIDED_COURSE_ARROW:?})"
                ));
            }
        };
    let mut validated_style = LocationIndicatorStyle::with_defaults(visual_kind);
    if let Some(radius_pixels) =
        optional_javascript_number_field(style_object, INDICATOR_FIELD_RADIUS_PIXELS)?
    {
        validated_positive_screen_pixels(radius_pixels, INDICATOR_FIELD_RADIUS_PIXELS)?;
        validated_style.radius_pixels = Some(radius_pixels);
    }
    if let Some(rgba_color) =
        optional_javascript_color_field(style_object, INDICATOR_FIELD_RGBA_COLOR)?
    {
        validated_style.rgba_color = Some(rgba_color);
    }
    if let Some(ring_rgba_color) =
        optional_javascript_color_field(style_object, INDICATOR_FIELD_RING_RGBA_COLOR)?
    {
        validated_style.ring_rgba_color = Some(ring_rgba_color);
    }
    if let Some(halo_rgba_color) =
        optional_javascript_color_field(style_object, INDICATOR_FIELD_HALO_RGBA_COLOR)?
    {
        validated_style.halo_rgba_color = Some(halo_rgba_color);
    }
    if let Some(show_accuracy_ring) =
        optional_javascript_boolean_field(style_object, INDICATOR_FIELD_SHOW_ACCURACY_RING)?
    {
        validated_style.show_accuracy_ring = Some(show_accuracy_ring);
    }
    Ok(validated_style)
}

/// Whole-number upper bound of a `wasm-bindgen` enum value inside a plain
/// JavaScript object field.
pub(super) const MAXIMUM_ENUM_DISCRIMINANT: f64 = u32::MAX as f64;

/// Parses a plain JavaScript number carrying a `wasm-bindgen` enum value
/// (the enum's numeric discriminant) into its whole `u32`, rejecting
/// anything else.
pub(super) fn validated_javascript_discriminant(javascript_value: &JsValue) -> Result<u32, String> {
    match javascript_value.as_f64() {
        Some(discriminant_number)
            if discriminant_number.is_finite()
                && discriminant_number >= 0.0
                && discriminant_number == discriminant_number.trunc()
                && discriminant_number <= MAXIMUM_ENUM_DISCRIMINANT =>
        {
            Ok(discriminant_number as u32)
        }
        _ => Err("the value is not a whole wasm-bindgen enum discriminant".to_string()),
    }
}
