//! CSS colour parsing and interpolation.
//!
//! Colours are stored as sRGB components and alpha in the `0.0..=1.0` range,
//! the representation the renderer's existing paint values use. The parser
//! accepts the forms a style document carries: `#rgb`, `#rgba`, `#rrggbb`,
//! `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()`, `hsla()` and the named colours the
//! shipped styles use.
//!
//! Interpolation is channel-wise sRGB interpolation without premultiplied
//! alpha, matching MapLibre's default `interpolate` colour space.

use crate::error::ColorParseError;

/// The maximum value of an `rgb()` channel.
const RGB_CHANNEL_MAXIMUM: f64 = 255.0;
/// The maximum value of an alpha channel.
const ALPHA_MAXIMUM: f64 = 1.0;
/// The divisor that turns a CSS percentage into a fraction.
const PERCENTAGE_DIVISOR: f64 = 100.0;
/// The radix of a hexadecimal colour channel.
const HEXADECIMAL_RADIX: u32 = 16;
/// The number of digits of a `#rrggbb` colour.
const SIX_DIGIT_HEXADECIMAL_LENGTH: usize = 6;
/// The number of digits of a `#rrggbbaa` colour.
const EIGHT_DIGIT_HEXADECIMAL_LENGTH: usize = 8;
/// The number of digits of a `#rgb` colour.
const THREE_DIGIT_HEXADECIMAL_LENGTH: usize = 3;
/// The number of digits of a `#rgba` colour.
const FOUR_DIGIT_HEXADECIMAL_LENGTH: usize = 4;
/// The shift that expands one hexadecimal nibble to a full byte.
const HEXADECIMAL_NIBBLE_EXPANSION: u32 = 17;
/// A full turn in degrees, the period of an HSL hue.
const HUE_PERIOD_DEGREES: f64 = 360.0;
/// The hue-sector offset used by the CSS Color 4 HSL formula.
const HSL_SECTOR_OFFSET_DEGREES: f64 = 30.0;
/// The number of sectors in the HSL hue circle.
const HSL_SECTOR_COUNT: f64 = 12.0;
/// The low sector pivot of the CSS Color 4 HSL formula.
const HSL_LOW_SECTOR_PIVOT: f64 = 3.0;
/// The high sector pivot of the CSS Color 4 HSL formula.
const HSL_HIGH_SECTOR_PIVOT: f64 = 9.0;
/// The channel offset that produces red in the CSS Color 4 HSL formula.
const HSL_RED_CHANNEL_OFFSET: f64 = 0.0;
/// The channel offset that produces green in the CSS Color 4 HSL formula.
const HSL_GREEN_CHANNEL_OFFSET: f64 = 8.0;
/// The channel offset that produces blue in the CSS Color 4 HSL formula.
const HSL_BLUE_CHANNEL_OFFSET: f64 = 4.0;
/// The saturation/lightness divisor of a CSS percentage.
const HSL_PERCENTAGE_DIVISOR: f64 = 100.0;

/// An sRGB colour with a non-premultiplied alpha channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Red component in `0.0..=1.0`.
    pub red: f64,
    /// Green component in `0.0..=1.0`.
    pub green: f64,
    /// Blue component in `0.0..=1.0`.
    pub blue: f64,
    /// Alpha component in `0.0..=1.0`.
    pub alpha: f64,
}

impl Color {
    /// Opaque black, the specification's default paint for most properties.
    pub const BLACK: Self = Self {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
        alpha: 1.0,
    };

    /// Opaque white.
    pub const WHITE: Self = Self {
        red: 1.0,
        green: 1.0,
        blue: 1.0,
        alpha: 1.0,
    };

    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
        alpha: 0.0,
    };

    /// Builds a colour from four `0.0..=1.0` components, clamping each.
    pub fn new(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
        Self {
            red: red.clamp(0.0, 1.0),
            green: green.clamp(0.0, 1.0),
            blue: blue.clamp(0.0, 1.0),
            alpha: alpha.clamp(0.0, 1.0),
        }
    }

    /// Builds a colour from `0..=255` channel values and a `0.0..=1.0`
    /// alpha, the form the `rgb` and `rgba` expressions use.
    pub fn from_rgb_numbers(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
        Self::new(
            red / RGB_CHANNEL_MAXIMUM,
            green / RGB_CHANNEL_MAXIMUM,
            blue / RGB_CHANNEL_MAXIMUM,
            alpha,
        )
    }

    /// Parses a CSS colour string.
    ///
    /// # Errors
    /// A malformed hexadecimal literal, a function argument that is not a
    /// number or percentage, an unknown function, or an unknown colour name
    /// reports [`ColorParseError`].
    pub fn from_css(text: &str) -> Result<Self, ColorParseError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(ColorParseError::new(text, "the colour is empty"));
        }
        if let Some(hexadecimal) = trimmed.strip_prefix('#') {
            return Self::from_hexadecimal(trimmed, hexadecimal);
        }
        let lowercased = trimmed.to_ascii_lowercase();
        if lowercased.starts_with("rgb(") || lowercased.starts_with("rgba(") {
            return Self::from_rgb_function(trimmed);
        }
        if lowercased.starts_with("hsl(") || lowercased.starts_with("hsla(") {
            return Self::from_hsl_function(trimmed);
        }
        Self::from_named(trimmed, &lowercased)
    }

    /// The colour's channels as 8-bit values.
    pub fn to_rgba8(self) -> [u8; 4] {
        [
            channel_to_u8(self.red),
            channel_to_u8(self.green),
            channel_to_u8(self.blue),
            channel_to_u8(self.alpha),
        ]
    }

    /// The colour as `#rrggbb` (opaque) or `#rrggbbaa` (translucent).
    pub fn to_css_string(self) -> String {
        let [red, green, blue, alpha] = self.to_rgba8();
        if alpha == u8::MAX {
            format!("#{red:02x}{green:02x}{blue:02x}")
        } else {
            format!("#{red:02x}{green:02x}{blue:02x}{alpha:02x}")
        }
    }

    /// Interpolates between two colours channel by channel in sRGB space.
    pub fn interpolate(from: Self, to: Self, fraction: f64) -> Self {
        let blend = |start: f64, end: f64| start + (end - start) * fraction;
        Self::new(
            blend(from.red, to.red),
            blend(from.green, to.green),
            blend(from.blue, to.blue),
            blend(from.alpha, to.alpha),
        )
    }

    fn from_hexadecimal(original: &str, hexadecimal: &str) -> Result<Self, ColorParseError> {
        let channel = |start: usize, end: usize| -> Result<u8, ColorParseError> {
            let pair = hexadecimal.get(start..end).ok_or_else(|| {
                ColorParseError::new(original, "a hexadecimal colour has an incomplete channel")
            })?;
            u8::from_str_radix(pair, HEXADECIMAL_RADIX).map_err(|_| {
                ColorParseError::new(original, "a hexadecimal colour channel is not hexadecimal")
            })
        };
        match hexadecimal.len() {
            THREE_DIGIT_HEXADECIMAL_LENGTH | FOUR_DIGIT_HEXADECIMAL_LENGTH => {
                let nibble = |index: usize| -> Result<u8, ColorParseError> {
                    let digit = hexadecimal.get(index..index + 1).ok_or_else(|| {
                        ColorParseError::new(
                            original,
                            "a hexadecimal colour has an incomplete channel",
                        )
                    })?;
                    let value = u8::from_str_radix(digit, HEXADECIMAL_RADIX).map_err(|_| {
                        ColorParseError::new(
                            original,
                            "a hexadecimal colour channel is not hexadecimal",
                        )
                    })?;
                    Ok((u32::from(value) * HEXADECIMAL_NIBBLE_EXPANSION) as u8)
                };
                let red = nibble(0)?;
                let green = nibble(1)?;
                let blue = nibble(2)?;
                let alpha = if hexadecimal.len() == FOUR_DIGIT_HEXADECIMAL_LENGTH {
                    nibble(3)?
                } else {
                    u8::MAX
                };
                Ok(Self::from_rgba8(red, green, blue, alpha))
            }
            SIX_DIGIT_HEXADECIMAL_LENGTH | EIGHT_DIGIT_HEXADECIMAL_LENGTH => {
                let red = channel(0, 2)?;
                let green = channel(2, 4)?;
                let blue = channel(4, 6)?;
                let alpha = if hexadecimal.len() == EIGHT_DIGIT_HEXADECIMAL_LENGTH {
                    channel(6, 8)?
                } else {
                    u8::MAX
                };
                Ok(Self::from_rgba8(red, green, blue, alpha))
            }
            _ => Err(ColorParseError::new(
                original,
                "a hexadecimal colour has 3, 4, 6 or 8 digits",
            )),
        }
    }

    fn from_rgba8(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red: f64::from(red) / RGB_CHANNEL_MAXIMUM,
            green: f64::from(green) / RGB_CHANNEL_MAXIMUM,
            blue: f64::from(blue) / RGB_CHANNEL_MAXIMUM,
            alpha: f64::from(alpha) / RGB_CHANNEL_MAXIMUM,
        }
    }

    fn from_rgb_function(original: &str) -> Result<Self, ColorParseError> {
        let arguments = split_function_arguments(original)?;
        if arguments.len() != 3 && arguments.len() != 4 {
            return Err(ColorParseError::new(
                original,
                "rgb() and rgba() take three channels and an optional alpha",
            ));
        }
        let red = parse_rgb_channel(original, &arguments[0])?;
        let green = parse_rgb_channel(original, &arguments[1])?;
        let blue = parse_rgb_channel(original, &arguments[2])?;
        let alpha = match arguments.get(3) {
            Some(argument) => parse_alpha_channel(original, argument)?,
            None => ALPHA_MAXIMUM,
        };
        Ok(Self::from_rgb_numbers(red, green, blue, alpha))
    }

    fn from_hsl_function(original: &str) -> Result<Self, ColorParseError> {
        let arguments = split_function_arguments(original)?;
        if arguments.len() != 3 && arguments.len() != 4 {
            return Err(ColorParseError::new(
                original,
                "hsl() and hsla() take hue, saturation, lightness and an optional alpha",
            ));
        }
        let hue = parse_hue(original, &arguments[0])?;
        let saturation = parse_percentage(original, &arguments[1])?;
        let lightness = parse_percentage(original, &arguments[2])?;
        let alpha = match arguments.get(3) {
            Some(argument) => parse_alpha_channel(original, argument)?,
            None => ALPHA_MAXIMUM,
        };
        let [red, green, blue] = hsl_to_rgb(hue, saturation, lightness);
        Ok(Self::new(red, green, blue, alpha))
    }

    fn from_named(original: &str, lowercased: &str) -> Result<Self, ColorParseError> {
        let color = match lowercased {
            "transparent" => Color::TRANSPARENT,
            "black" => Color::BLACK,
            "silver" => Color::from_rgba8(192, 192, 192, 255),
            "gray" | "grey" => Color::from_rgba8(128, 128, 128, 255),
            "white" => Color::WHITE,
            "maroon" => Color::from_rgba8(128, 0, 0, 255),
            "red" => Color::from_rgba8(255, 0, 0, 255),
            "purple" => Color::from_rgba8(128, 0, 128, 255),
            "fuchsia" | "magenta" => Color::from_rgba8(255, 0, 255, 255),
            "green" => Color::from_rgba8(0, 128, 0, 255),
            "lime" => Color::from_rgba8(0, 255, 0, 255),
            "olive" => Color::from_rgba8(128, 128, 0, 255),
            "yellow" => Color::from_rgba8(255, 255, 0, 255),
            "navy" => Color::from_rgba8(0, 0, 128, 255),
            "blue" => Color::from_rgba8(0, 0, 255, 255),
            "teal" => Color::from_rgba8(0, 128, 128, 255),
            "aqua" | "cyan" => Color::from_rgba8(0, 255, 255, 255),
            "orange" => Color::from_rgba8(255, 165, 0, 255),
            "rebeccapurple" => Color::from_rgba8(102, 51, 153, 255),
            "darkslategray" | "darkslategrey" => Color::from_rgba8(47, 79, 79, 255),
            "dimgray" | "dimgrey" => Color::from_rgba8(105, 105, 105, 255),
            "lightgray" | "lightgrey" => Color::from_rgba8(211, 211, 211, 255),
            "whitesmoke" => Color::from_rgba8(245, 245, 245, 255),
            "gainsboro" => Color::from_rgba8(220, 220, 220, 255),
            "beige" => Color::from_rgba8(245, 245, 220, 255),
            "ivory" => Color::from_rgba8(255, 255, 240, 255),
            "khaki" => Color::from_rgba8(240, 230, 140, 255),
            "tan" => Color::from_rgba8(210, 180, 140, 255),
            "brown" => Color::from_rgba8(165, 42, 42, 255),
            "coral" => Color::from_rgba8(255, 127, 80, 255),
            "salmon" => Color::from_rgba8(250, 128, 114, 255),
            "gold" => Color::from_rgba8(255, 215, 0, 255),
            "pink" => Color::from_rgba8(255, 192, 203, 255),
            "violet" => Color::from_rgba8(238, 130, 238, 255),
            "indigo" => Color::from_rgba8(75, 0, 130, 255),
            "steelblue" => Color::from_rgba8(70, 130, 180, 255),
            "skyblue" => Color::from_rgba8(135, 206, 235, 255),
            "royalblue" => Color::from_rgba8(65, 105, 225, 255),
            "forestgreen" => Color::from_rgba8(34, 139, 34, 255),
            "seagreen" => Color::from_rgba8(46, 139, 87, 255),
            _ => {
                return Err(ColorParseError::new(
                    original,
                    "the colour name is not one this parser knows",
                ));
            }
        };
        Ok(color)
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

fn channel_to_u8(channel: f64) -> u8 {
    (channel.clamp(0.0, 1.0) * RGB_CHANNEL_MAXIMUM).round() as u8
}

/// Splits the arguments of a CSS colour function, accepting both the
/// comma-separated and the space-separated (with an optional `/` alpha)
/// spellings.
fn split_function_arguments(original: &str) -> Result<Vec<String>, ColorParseError> {
    let opening = original.find('(').ok_or_else(|| {
        ColorParseError::new(original, "a colour function needs an opening parenthesis")
    })?;
    let body = original
        .get(opening + 1..)
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| {
            ColorParseError::new(original, "a colour function needs a closing parenthesis")
        })?;
    Ok(body
        .split(|character: char| character == ',' || character == '/' || character.is_whitespace())
        .map(str::trim)
        .filter(|argument| !argument.is_empty())
        .map(str::to_string)
        .collect())
}

fn parse_rgb_channel(original: &str, argument: &str) -> Result<f64, ColorParseError> {
    if let Some(percentage) = argument.strip_suffix('%') {
        let value = percentage.trim().parse::<f64>().map_err(|_| {
            ColorParseError::new(original, format!("{argument:?} is not a percentage"))
        })?;
        return Ok(
            (value / PERCENTAGE_DIVISOR * RGB_CHANNEL_MAXIMUM).clamp(0.0, RGB_CHANNEL_MAXIMUM)
        );
    }
    argument
        .parse::<f64>()
        .map(|value| value.clamp(0.0, RGB_CHANNEL_MAXIMUM))
        .map_err(|_| ColorParseError::new(original, format!("{argument:?} is not a number")))
}

fn parse_alpha_channel(original: &str, argument: &str) -> Result<f64, ColorParseError> {
    if let Some(percentage) = argument.strip_suffix('%') {
        let value = percentage.trim().parse::<f64>().map_err(|_| {
            ColorParseError::new(original, format!("{argument:?} is not a percentage"))
        })?;
        return Ok((value / PERCENTAGE_DIVISOR).clamp(0.0, ALPHA_MAXIMUM));
    }
    argument
        .parse::<f64>()
        .map(|value| value.clamp(0.0, ALPHA_MAXIMUM))
        .map_err(|_| ColorParseError::new(original, format!("{argument:?} is not a number")))
}

fn parse_percentage(original: &str, argument: &str) -> Result<f64, ColorParseError> {
    let number = argument.trim_end_matches('%');
    number
        .parse::<f64>()
        .map(|value| value.clamp(0.0, HSL_PERCENTAGE_DIVISOR))
        .map_err(|_| ColorParseError::new(original, format!("{argument:?} is not a percentage")))
}

fn parse_hue(original: &str, argument: &str) -> Result<f64, ColorParseError> {
    let number = argument.strip_suffix("deg").unwrap_or(argument).trim();
    let hue = number
        .parse::<f64>()
        .map_err(|_| ColorParseError::new(original, format!("{argument:?} is not a hue")))?;
    Ok(hue.rem_euclid(HUE_PERIOD_DEGREES))
}

fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> [f64; 3] {
    let saturation = saturation / HSL_PERCENTAGE_DIVISOR;
    let lightness = lightness / HSL_PERCENTAGE_DIVISOR;
    let channel = |offset: f64| {
        let sector = (offset + hue / HSL_SECTOR_OFFSET_DEGREES) % HSL_SECTOR_COUNT;
        let amount = saturation * lightness.min(1.0 - lightness);
        lightness
            - amount
                * (-1.0_f64)
                    .max((sector - HSL_LOW_SECTOR_PIVOT).min(HSL_HIGH_SECTOR_PIVOT - sector))
                    .min(1.0)
    };
    [
        channel(HSL_RED_CHANNEL_OFFSET),
        channel(HSL_GREEN_CHANNEL_OFFSET),
        channel(HSL_BLUE_CHANNEL_OFFSET),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn components(color: Color) -> [u8; 4] {
        color.to_rgba8()
    }

    #[test]
    fn hexadecimal_forms_parse() {
        assert_eq!(components(Color::from_css("#fff").unwrap()), [255; 4]);
        assert_eq!(
            components(Color::from_css("#1a2b3c").unwrap()),
            [0x1a, 0x2b, 0x3c, 0xff]
        );
        assert_eq!(
            components(Color::from_css("#1a2b3c80").unwrap()),
            [0x1a, 0x2b, 0x3c, 0x80]
        );
        assert_eq!(
            components(Color::from_css("#1234").unwrap()),
            [0x11, 0x22, 0x33, 0x44]
        );
    }

    #[test]
    fn rgb_rgba_and_hsl_functions_parse() {
        assert_eq!(
            components(Color::from_css("rgb(255, 0, 128)").unwrap()),
            [255, 0, 128, 255]
        );
        assert_eq!(
            components(Color::from_css("rgba(255, 0, 128, 0.5)").unwrap()),
            [255, 0, 128, 128]
        );
        assert_eq!(
            components(Color::from_css("rgb(100% 0% 50% / 25%)").unwrap()),
            [255, 0, 128, 64]
        );
        let red = Color::from_css("hsl(0, 100%, 50%)").unwrap();
        assert_eq!(components(red), [255, 0, 0, 255]);
        let green = Color::from_css("hsla(120, 100%, 50%, 0.5)").unwrap();
        assert_eq!(components(green), [0, 255, 0, 128]);
    }

    #[test]
    fn named_colours_parse() {
        assert_eq!(components(Color::from_css("white").unwrap()), [255; 4]);
        assert_eq!(
            components(Color::from_css("transparent").unwrap()),
            [0, 0, 0, 0]
        );
        assert_eq!(
            components(Color::from_css("Red").unwrap()),
            [255, 0, 0, 255]
        );
        assert!(Color::from_css("not-a-colour").is_err());
    }

    #[test]
    fn interpolation_is_channel_wise() {
        let blended = Color::interpolate(Color::BLACK, Color::WHITE, 0.5);
        assert_eq!(blended, Color::new(0.5, 0.5, 0.5, 1.0));
    }
}
