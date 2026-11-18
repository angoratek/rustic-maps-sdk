//! The reference application's theme choice: three explicit choices —
//! Light, Dark and Auto — with Auto the default on a device that never chose.
//! Auto follows the operating system's color scheme live; an explicit Light
//! or Dark choice is remembered on the device and wins from then on. The
//! choice itself never leaves the device.

import Foundation
import SwiftUI
import MapNativeCoreUI

/// The three-way theme choice (§3): the state the toggle cycles through and
/// the device stores, kept apart from the resolved `AppTheme` the tokens
/// render against.
enum ThemeChoice: String, CaseIterable, Sendable {
    /// The light presentation: light panels over the dark canvas.
    case light
    /// The dark presentation: dark translucent panels over the dark canvas.
    case dark
    /// The operating system's choice, followed live; the default state of a
    /// device that never chose.
    case auto

    /// The device-local `UserDefaults` key an explicit choice is remembered
    /// under. Nothing about the theme leaves the device.
    static let storedChoiceDefaultsKey = "rusticMapsSdk.themeChoice"

    /// The choice the toggle cycles to (light → dark → auto → light).
    var nextChoice: ThemeChoice {
        switch self {
        case .light: return .dark
        case .dark: return .auto
        case .auto: return .light
        }
    }

    /// The toggle button's label, always naming the choice it switches to
    /// next ("Switch to auto theme", §3).
    var switchControlLabel: String {
        "Switch to \(nextChoice.rawValue) theme"
    }

    /// The resolved presentation theme: the choice itself for Light and
    /// Dark, the operating system's color scheme while Auto.
    func resolvedTheme(systemColorScheme: ColorScheme) -> AppTheme {
        switch self {
        case .light: return .light
        case .dark: return .dark
        case .auto: return systemColorScheme == .dark ? .dark : .light
        }
    }

    /// The choice this device remembered, or Auto when it never chose (§3).
    static func storedChoice(in defaults: UserDefaults = .standard) -> ThemeChoice {
        defaults.string(forKey: storedChoiceDefaultsKey)
            .flatMap(ThemeChoice.init(rawValue:)) ?? .auto
    }

    /// Remembers this explicit choice on the device (the Auto state is stored
    /// too: choosing Auto after a Light or Dark choice is itself a choice).
    func store(in defaults: UserDefaults = .standard) {
        defaults.set(rawValue, forKey: Self.storedChoiceDefaultsKey)
    }

    /// Clears the legacy theme entry when the shared record is reset.
    static func clearStoredChoice(in defaults: UserDefaults = .standard) {
        defaults.removeObject(forKey: storedChoiceDefaultsKey)
    }
}
