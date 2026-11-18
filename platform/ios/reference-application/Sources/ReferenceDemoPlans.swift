//! The demo-plan picker's plans (docs/ReferenceAppUX.md §4.1): one
//! selectable plan per committed recorded trace fixture, the shared demo
//! artifact's own navigation demo trace selected by default. The artifact's
//! `demo_plans` section names each plan's scenario, fixture, label and
//! info-affordance description (the product-language rule: the rows name
//! the feature, never the seam), so the rows cannot drift from the ones the
//! Android and React Native reference applications show. The loading is
//! network-free — the committed fixtures are the single sources, bundled
//! straight from them.

import Foundation
import MapNativeCoreUI

/// One selectable demo plan of the home sheet's demo-plan picker: a decoded
/// recorded trace with its consumer-facing presentation (the same picker
/// the Android and React Native reference applications render).
struct ReferenceDemoPlan: Identifiable {
    /// The fixture metadata's own `scenario_name` (the plan's identifier;
    /// diagnostics detail, never a chip label).
    let scenarioName: String
    /// The consumer-facing plan name (the chip's label).
    let label: String
    /// The description behind the plan's info affordance.
    let descriptionText: String
    /// The decoded recorded trace the plan replays.
    let journey: RecordedTraceJourney
    /// Whether this plan is the shared demo artifact's own navigation demo
    /// trace (the picker's default selection).
    let isArtifactTrace: Bool

    var id: String { scenarioName }
}

/// The demo plans' loading from the application bundle. Every failure
/// throws loudly (the readiness ladder reports it verbatim, §6.3) — the
/// picker never invents labels or falls back to other plans.
enum ReferenceDemoPlanLoader {
    /// A demo-plan loading failure (the ladder's failure text renders it).
    enum ReferenceDemoPlanError: Swift.Error, LocalizedError {
        /// The fixture is not bundled under its documented asset name.
        case fixtureMissing(assetName: String)
        /// The fixture could not be read from the bundle.
        case fixtureUnreadable(assetName: String, underlying: Swift.Error)
        /// The bundled fixture's own scenario name disagrees with the
        /// artifact's plan definition (the artifacts drifted apart).
        case scenarioMismatch(
            assetName: String,
            expectedScenario: String,
            actualScenario: String
        )

        var errorDescription: String? {
            switch self {
            case .fixtureMissing(let assetName):
                return "the recorded trace fixture '\(assetName)' is not bundled"
            case .fixtureUnreadable(let assetName, _):
                return "the recorded trace fixture '\(assetName)' could not be read"
            case .scenarioMismatch(let assetName, let expectedScenario, let actualScenario):
                return """
                    the recorded trace fixture '\(assetName)' replays scenario \
                    '\(actualScenario)', but the demo artifact names '\(expectedScenario)'
                    """
            }
        }
    }

    /// Loads every recorded demo trace fixture named by the shared artifact's
    /// `demo_plans` section, in artifact order.
    static func plansFromBundle(catalog: ReferenceScenesCatalog) throws -> [ReferenceDemoPlan] {
        try catalog.demoPlans.map { planDefinition in
            let assetName = planDefinition.traceAssetName
            let fixtureResourceName = (assetName as NSString).deletingPathExtension
            let fixtureResourceExtension = (assetName as NSString).pathExtension
            guard let fixtureURL = Bundle.main.url(
                forResource: fixtureResourceName,
                withExtension: fixtureResourceExtension
            ) else {
                throw ReferenceDemoPlanError.fixtureMissing(assetName: assetName)
            }
            let fixtureData: Data
            do {
                fixtureData = try Data(contentsOf: fixtureURL)
            } catch {
                throw ReferenceDemoPlanError.fixtureUnreadable(
                    assetName: assetName,
                    underlying: error
                )
            }
            let fixtureScenarioName = try scenarioName(fromFixtureData: fixtureData)
            guard fixtureScenarioName == planDefinition.scenario else {
                throw ReferenceDemoPlanError.scenarioMismatch(
                    assetName: assetName,
                    expectedScenario: planDefinition.scenario,
                    actualScenario: fixtureScenarioName
                )
            }
            return ReferenceDemoPlan(
                scenarioName: fixtureScenarioName,
                label: planDefinition.label,
                descriptionText: planDefinition.description,
                journey: try RecordedTraceJourney.decoded(from: fixtureData),
                isArtifactTrace:
                    fixtureScenarioName == catalog.navigationDemo.simulatedTrace.name
            )
        }
    }

    /// The fixture's metadata `scenario_name` (a metadata-only decode; the
    /// journey decode owns the route and the samples).
    static func scenarioName(fromFixtureData fixtureData: Data) throws -> String {
        let metadataDecoder = JSONDecoder()
        metadataDecoder.keyDecodingStrategy = .convertFromSnakeCase
        return try metadataDecoder
            .decode(RecordedTraceMetadata.self, from: fixtureData)
            .metadata.scenarioName
    }

    /// The recorded fixture's metadata fields the picker consumes (private
    /// to this module; the full document belongs to the journey decode).
    private struct RecordedTraceMetadata: Decodable {
        let metadata: Metadata

        struct Metadata: Decodable {
            let scenarioName: String
        }
    }
}