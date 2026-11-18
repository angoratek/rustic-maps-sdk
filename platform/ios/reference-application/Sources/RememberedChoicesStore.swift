//! The explicit choices the reference application remembers across launches
//! (PRD.md §6 item 67): the theme, the base map, the
//! demo plan and overlay scenes, the camera preset, the simulated-trip speed
//! multiplier, the last search texts and the provider chosen for each
//! capability. Transient state — the open sheet, the current camera pose, a
//! route result — is never part of this record, and the provider credential
//! stays in its own opt-in store.

import Foundation

/// One remembered record. Every field is an explicit user choice or the text
/// the user typed; the defaults describe a device that never chose.
struct RememberedChoices: Equatable {
    /// The three-way theme choice (§3).
    var theme: ThemeChoice = .auto
    /// The base map's provider key, or nil while none was chosen.
    var baseMapProviderKey: String?
    /// The styled vector base's style id; nil names the provider's raster source.
    var baseMapStyleId: String?
    /// The demo-plan picker's selected scenario name.
    var demoPlanScenarioName: String?
    /// The enabled overlay-scene keys of the shared artifact.
    var sceneKeys: Set<String> = []
    /// Whether the traffic layer is on. A record written before the field
    /// existed decodes to off.
    var trafficLayerEnabled: Bool = false
    /// Whether the device location source is on. A record written before the
    /// field existed decodes to off.
    var deviceLocationEnabled: Bool = false
    /// The Directions transport mode; a missing or unknown value falls back
    /// to Car.
    var directionsMode: DirectionsMode = .car
    /// The Directions route strategy; a missing or unknown value falls back
    /// to Fastest.
    var directionsStrategy: DirectionsStrategy = .fastest
    /// The selected camera preset's identifier.
    var cameraPresetIdentifier: String?
    /// The simulated-trip playback speed multiplier.
    var simulatedTripSpeedMultiplier: Double?
    /// The origin field's last text.
    var originSearchText: String = ""
    /// The destination field's last text.
    var destinationSearchText: String = ""
    /// The last text submitted to the plan search.
    var lastSearchQueryText: String = ""
    /// The provider key chosen for each capability, keyed by the contract's
    /// stable storage name (`Routing`, `DistanceMatrix`, …). Absent from a
    /// record written before per-capability choices existed, so such a record
    /// still loads and yields an empty map.
    var capabilityProviders: [String: String] = [:]
}

/// The device-local store of `RememberedChoices`: one versioned JSON value in
/// the application's `UserDefaults`, written on every explicit choice and read
/// once at startup. A record written by another version is dropped rather than
/// guessed at, and unknown keys are ignored.
final class RememberedChoicesStore {
    /// The store the application model uses.
    static let standard = RememberedChoicesStore()

    /// The defaults key the record lives under.
    static let recordDefaultsKey = "rusticMapsSdk.rememberedChoices"

    /// The record schema version (a record of another version is dropped).
    static let recordVersion = 1

    private let defaults: UserDefaults

    /// Creates a store over one defaults domain (the standard domain by default).
    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    /// The remembered record, or nil when this device never made one.
    func load() -> RememberedChoices? {
        guard let recordText = defaults.string(forKey: Self.recordDefaultsKey),
              let recordData = recordText.data(using: .utf8),
              let jsonObject = try? JSONSerialization.jsonObject(with: recordData),
              let record = jsonObject as? [String: Any],
              let version = record["version"] as? Int,
              version == Self.recordVersion
        else {
            return nil
        }
        let baseMap = record["baseMap"] as? [String: Any]
        let demoPlan = record["demoPlan"] as? [String: Any]
        let cameraPreset = record["cameraPreset"] as? [String: Any]
        let search = record["search"] as? [String: Any]
        let capabilityProviders = (record["capabilityProviders"] as? [String: Any] ?? [:])
            .compactMapValues { providerKey in
                nonEmptyString(providerKey)
            }
        let themeText = record["theme"] as? String
        return RememberedChoices(
            theme: themeText.flatMap(ThemeChoice.init(rawValue:)) ?? .auto,
            baseMapProviderKey: nonEmptyString(baseMap?["providerKey"]),
            baseMapStyleId: nonEmptyString(baseMap?["styleId"]),
            demoPlanScenarioName: nonEmptyString(demoPlan?["scenarioName"]),
            sceneKeys: Set(
                (record["scenes"] as? [Any] ?? [])
                    .compactMap { sceneKey in sceneKey as? String }
                    .filter { sceneKey in !sceneKey.isEmpty }
            ),
            trafficLayerEnabled: record["trafficLayerEnabled"] as? Bool ?? false,
            deviceLocationEnabled: record["deviceLocationEnabled"] as? Bool ?? false,
            directionsMode: DirectionsMode(
                recordValue: record["directionsTransportationMode"] as? String
            ) ?? .car,
            directionsStrategy: DirectionsStrategy(
                recordValue: record["directionsRouteStrategy"] as? String
            ) ?? .fastest,
            cameraPresetIdentifier: nonEmptyString(cameraPreset?["id"]),
            simulatedTripSpeedMultiplier: record["simulatedTripSpeedMultiplier"] as? Double,
            originSearchText: search?["origin"] as? String ?? "",
            destinationSearchText: search?["destination"] as? String ?? "",
            lastSearchQueryText: search?["query"] as? String ?? "",
            capabilityProviders: capabilityProviders
        )
    }

    /// Remembers the complete record, replacing any earlier one.
    func store(_ choices: RememberedChoices) {
        var record: [String: Any] = [
            "version": Self.recordVersion,
            "theme": choices.theme.rawValue,
            "scenes": choices.sceneKeys.sorted(),
            "trafficLayerEnabled": choices.trafficLayerEnabled,
            "deviceLocationEnabled": choices.deviceLocationEnabled,
            "directionsTransportationMode": choices.directionsMode.recordValue,
            "directionsRouteStrategy": choices.directionsStrategy.recordValue,
            "search": [
                "origin": choices.originSearchText,
                "destination": choices.destinationSearchText,
                "query": choices.lastSearchQueryText,
            ],
        ]
        if let providerKey = choices.baseMapProviderKey {
            var baseMap: [String: Any] = ["providerKey": providerKey]
            if let styleId = choices.baseMapStyleId {
                baseMap["styleId"] = styleId
            }
            record["baseMap"] = baseMap
        }
        if let scenarioName = choices.demoPlanScenarioName {
            record["demoPlan"] = ["scenarioName": scenarioName]
        }
        if let presetIdentifier = choices.cameraPresetIdentifier {
            record["cameraPreset"] = ["id": presetIdentifier]
        }
        if let speedMultiplier = choices.simulatedTripSpeedMultiplier {
            record["simulatedTripSpeedMultiplier"] = speedMultiplier
        }
        if !choices.capabilityProviders.isEmpty {
            record["capabilityProviders"] = choices.capabilityProviders
        }
        guard let recordData = try? JSONSerialization.data(
            withJSONObject: record,
            options: [.sortedKeys]
        ) else {
            return
        }
        defaults.set(String(decoding: recordData, as: UTF8.self), forKey: Self.recordDefaultsKey)
    }

    /// Clears the remembered record; the credential store is separate.
    func clear() {
        defaults.removeObject(forKey: Self.recordDefaultsKey)
    }

    /// A decoded text field only when it carries a non-empty value.
    private func nonEmptyString(_ value: Any?) -> String? {
        guard let text = value as? String, !text.isEmpty else { return nil }
        return text
    }
}
