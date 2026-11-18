// The opt-in capture of the published iOS reference-application
//! screenshots: the plan sheet with both endpoints chosen, the provider
//! section with the applied provider serving every capability, the navigation
//! frame at the first instruction with the 1× speed selected, the styled
//! vector base with its place labels, and the layers control closed and open
//! over the credential's raster base.
//!
//! The capture is opt-in and needs a session credential because the provider
//! section documents the applied state:
//!
//! ```
//! TEST_RUNNER_CAPTURE_REFERENCE_SCREENSHOTS=1 \
//! TEST_RUNNER_PROVIDER_API_KEY=… \
//! TEST_RUNNER_PROVIDER_KEY=mapquest \
//! xcodebuild test -project ReferenceApplication.xcodeproj \
//!   -scheme ReferenceApplication -sdk iphonesimulator \
//!   -destination 'platform=iOS Simulator,name=iPhone 17 Pro' \
//!   -only-testing:ReferenceApplicationUITests/ReferenceApplicationScreenshotCaptureTest \
//!   -resultBundlePath tmp/reference-screenshots/ios/ReferenceApplicationScreenshots.xcresult
//! ```
//!
//! `xcodebuild` does not forward the shell environment to the UI-test runner,
//! so the `TEST_RUNNER_` prefix is required; the runner reads the unprefixed
//! names and forwards the credential to the application's launch environment
//! (the same runtime injection path the settings surface uses). The value is
//! never persisted and never logged, and the capture assertions never print
//! it. Screenshots are exported from the result bundle's attachments.

import XCTest

/// The four published captures from one keyed run.
final class ReferenceApplicationScreenshotCaptureTest: XCTestCase {
    /// The opt-in environment flag the runner reads (unprefixed).
    private static let captureEnvironmentKey = "CAPTURE_REFERENCE_SCREENSHOTS"

    /// The runtime credential the application's launch environment carries.
    private static let credentialEnvironmentKey = "PROVIDER_API_KEY"

    /// The optional provider key paired with the credential.
    private static let providerKeyEnvironmentKey = "PROVIDER_KEY"

    /// The readiness ladder's settled label with a serving provider.
    private static let readyChipLabel = "Ready"

    /// The published playback factor; all four platforms capture it.
    private static let publishedSpeedMultiplier = 1

    /// A fresh session's first drawn frames on the simulator.
    private static let firstFrameTimeoutSeconds: TimeInterval = 120

    /// A sheet interaction settles within this bound.
    private static let settleTimeoutSeconds: TimeInterval = 15

    /// The vector base's first labelled frame on the simulator.
    private static let vectorSettleSeconds: TimeInterval = 12

    /// One short settle after a driven state.
    private static let frameSettleSeconds: TimeInterval = 1.5

    /// The capture file names, named in page order.
    private static let planSheetFileName = "ios-plan-sheet"
    private static let providerSectionFileName = "ios-provider-section"
    private static let navigationFileName = "ios-navigation"
    private static let vectorBaseFileName = "ios-vector-base"
    private static let layersClosedFileName = "ios-layers-closed"
    private static let layersOpenFileName = "ios-layers-open"

    func testCapturesTheKeyedScreenshotSet() throws {
        let environment = ProcessInfo.processInfo.environment
        try XCTSkipUnless(
            environment[Self.captureEnvironmentKey] == "1",
            "screenshot capture is opt-in",
        )
        let runtimeCredential = environment[Self.credentialEnvironmentKey]?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        XCTAssertFalse(
            runtimeCredential?.isEmpty ?? true,
            "the keyed capture needs a session credential in "
                + Self.credentialEnvironmentKey,
        )
        let runtimeProviderKey = environment[Self.providerKeyEnvironmentKey]
            .map { providerKey in
                providerKey.trimmingCharacters(in: .whitespacesAndNewlines)
            }
            .flatMap { providerKey in providerKey.isEmpty ? nil : providerKey }

        try capturePlanSheet(
            credential: runtimeCredential ?? "",
            providerKey: runtimeProviderKey,
        )
        try captureProviderSection(
            credential: runtimeCredential ?? "",
            providerKey: runtimeProviderKey,
        )
        try captureVectorBase(
            credential: runtimeCredential ?? "",
            providerKey: runtimeProviderKey,
        )
        try captureNavigationFrame(
            credential: runtimeCredential ?? "",
            providerKey: runtimeProviderKey,
        )
    }

    // MARK: The four frames

    /// The plan sheet: both fields typed through the search view's own fields
    /// and the sheet left full.
    private func capturePlanSheet(
        credential: String,
        providerKey: String?,
    ) throws {
        // The capture flag fills both fields with the recorded route's
        // endpoint names through the application's own launch automation, so
        // no search provider call can replace them with a live guess.
        let application = launchApplication(
            launchArguments: ["-DemoCapturePlanEndpoints", "YES"],
            credential: credential,
            providerKey: providerKey,
        )
        awaitReady(application)
        let originField = application.textFields["Search an origin"]
        XCTAssertTrue(
            originField.waitForExistence(timeout: Self.firstFrameTimeoutSeconds),
            "the search view's origin field must render",
        )
        let destinationField = application.textFields["Search a destination"]
        XCTAssertTrue(
            destinationField.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the search view's destination field must render",
        )
        XCTAssertEqual(
            originField.value as? String,
            "Capitol Park",
            "the capture flag must fill the origin field",
        )
        XCTAssertEqual(
            destinationField.value as? String,
            "Tower Bridge",
            "the capture flag must fill the destination field",
        )
        Thread.sleep(forTimeInterval: Self.frameSettleSeconds)
        captureScreenshot(named: Self.planSheetFileName)
    }

    /// The provider section: the capability rows the SDK reports serving,
    /// every row's credential state, and the applied credential row.
    private func captureProviderSection(
        credential: String,
        providerKey: String?,
    ) throws {
        let application = launchApplication(
            launchArguments: [],
            credential: credential,
            providerKey: providerKey,
        )
        awaitReady(application)
        elementWithIdentifier("provider-settings-gear", in: application).tap()
        let settingsSheet = elementWithIdentifier(
            "provider-settings-sheet", in: application
        )
        XCTAssertTrue(
            settingsSheet.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the gear's tap must present the provider settings sheet",
        )
        // The capability rows sit below the tile and offline rows; a short
        // upward drag brings Geocoding through the traffic rows into the
        // phone-width frame with their composed credential states.
        dragSheetUp(byFraction: 0.18, in: settingsSheet)
        Thread.sleep(forTimeInterval: Self.frameSettleSeconds)
        captureScreenshot(named: Self.providerSectionFileName)
    }

    /// The styled vector base: the registry's first offered vector style,
    /// selected through the layers panel's own row act.
    private func captureVectorBase(
        credential: String,
        providerKey: String?,
    ) throws {
        let application = launchApplication(
            launchArguments: [],
            credential: credential,
            providerKey: providerKey,
        )
        awaitReady(application)
        elementWithIdentifier("map-layers-control", in: application).tap()
        let vectorStyleRows = application.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier BEGINSWITH %@", "map-layer-style-"))
        XCTAssertTrue(
            vectorStyleRows.firstMatch.waitForExistence(
                timeout: Self.settleTimeoutSeconds
            ),
            "the layers panel must offer a vector style row",
        )
        vectorStyleRows.firstMatch.tap()
        // Prove the frame draws the registry's styled base, not the raster
        // source the credential registered underneath it: the tapped style
        // row carries the panel's Serving mark.
        let servingPredicate = NSPredicate(format: "label CONTAINS %@", "Serving")
        let servingExpectation = expectation(
            for: servingPredicate, evaluatedWith: vectorStyleRows.firstMatch
        )
        wait(for: [servingExpectation], timeout: Self.settleTimeoutSeconds)
        elementWithIdentifier("map-layers-close", in: application).tap()
        Thread.sleep(forTimeInterval: Self.vectorSettleSeconds)
        captureScreenshot(named: Self.vectorBaseFileName)
    }

    /// The navigation frame: the recorded trace's first instruction with the
    /// published 1× speed selected.
    private func captureNavigationFrame(
        credential: String,
        providerKey: String?,
    ) throws {
        let application = launchApplication(
            launchArguments: ["-DemoStartSampleJourney", "YES"],
            credential: credential,
            providerKey: providerKey,
        )
        awaitReady(application)
        XCTAssertTrue(
            elementWithIdentifier("navigation-turn-banner", in: application)
                .waitForExistence(timeout: Self.firstFrameTimeoutSeconds),
            "the trip must present its turn banner",
        )
        let speedChip = elementWithIdentifier(
            "navigation-speed-chip-\(Self.publishedSpeedMultiplier)",
            in: application,
        )
        XCTAssertTrue(
            speedChip.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the drawer must offer the 1× speed chip",
        )
        speedChip.tap()
        XCTAssertTrue(
            elementWithIdentifier("navigation-drawer-eta-line", in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the drawer must present its ETA-distance line",
        )
        captureScreenshot(named: Self.navigationFileName)
    }

    /// The layers control pair from one keyed session on the credential's
    /// raster base: the map with the panel closed, then the same camera pose
    /// with the panel open over every section and the clear action.
    func testCapturesTheLayersPanelSet() throws {
        let environment = ProcessInfo.processInfo.environment
        try XCTSkipUnless(
            environment[Self.captureEnvironmentKey] == "1",
            "screenshot capture is opt-in",
        )
        let runtimeCredential = environment[Self.credentialEnvironmentKey]?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        XCTAssertFalse(
            runtimeCredential.isEmpty,
            "the keyed capture needs a session credential in "
                + Self.credentialEnvironmentKey,
        )
        let runtimeProviderKey = environment[Self.providerKeyEnvironmentKey]
            .map { providerKey in
                providerKey.trimmingCharacters(in: .whitespacesAndNewlines)
            }
            .flatMap { providerKey in providerKey.isEmpty ? nil : providerKey }

        let application = launchApplication(
            launchArguments: [],
            credential: runtimeCredential,
            providerKey: runtimeProviderKey,
        )
        awaitReady(application)
        let layersControl = elementWithIdentifier(
            "map-layers-control", in: application
        )
        XCTAssertTrue(
            layersControl.waitForExistence(timeout: Self.firstFrameTimeoutSeconds),
            "the collapsed layers control must render",
        )
        Thread.sleep(forTimeInterval: Self.frameSettleSeconds)
        captureScreenshot(named: Self.layersClosedFileName)

        layersControl.tap()
        let panel = elementWithIdentifier("map-layers-panel", in: application)
        XCTAssertTrue(
            panel.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the control's tap must present the layers panel",
        )
        // A raster base serves both frames. A remembered styled choice is
        // restored before the launch reset, so the raster row's own act
        // switches back to the credential provider's imagery.
        if let runtimeProviderKey {
            let rasterRow = elementWithIdentifier(
                "map-layer-base-\(runtimeProviderKey)", in: application
            )
            XCTAssertTrue(
                rasterRow.waitForExistence(timeout: Self.settleTimeoutSeconds),
                "the panel must offer the session provider's raster row",
            )
            if !rasterRow.label.contains("Serving") {
                rasterRow.tap()
            }
        }
        let servingRasterRow = application.descendants(matching: .any)
            .matching(
                NSPredicate(
                    format: "identifier BEGINSWITH %@ AND label CONTAINS %@",
                    "map-layer-base-",
                    "Serving"
                )
            )
        XCTAssertTrue(
            servingRasterRow.firstMatch.waitForExistence(
                timeout: Self.settleTimeoutSeconds
            ),
            "the panel must show the credential's raster base serving",
        )
        if let runtimeProviderKey {
            XCTAssertTrue(
                elementWithIdentifier(
                    "map-layer-base-\(runtimeProviderKey)", in: application
                ).exists,
                "the serving raster row must be the session's provider"
            )
        }
        for sectionTitle in ["Base map", "Traffic", "Overlays", "Device location"] {
            let sectionElement = application.descendants(matching: .any)
                .matching(NSPredicate(format: "label == %@", sectionTitle))
                .firstMatch
            XCTAssertTrue(
                sectionElement.waitForExistence(timeout: Self.settleTimeoutSeconds),
                "the panel must present the \(sectionTitle) section"
            )
        }
        XCTAssertTrue(
            elementWithIdentifier("map-layer-clear-overlays", in: application)
                .exists,
            "the overlays section must offer the shared clear action",
        )
        // The panel scrolls on a phone: the open frame shows its last base
        // rows with the traffic, overlays, clear and device-location sections
        // below them, so every section and the clear action are in one frame.
        dragSheetUp(byFraction: 0.5, in: panel)
        dragSheetUp(byFraction: 0.25, in: panel)
        Thread.sleep(forTimeInterval: Self.settleTimeoutSeconds / 3)
        XCTAssertTrue(
            elementWithIdentifier("device-location-status", in: application)
                .isHittable,
            "the device-location row must land in the open frame",
        )
        XCTAssertTrue(
            elementWithIdentifier("map-layer-clear-overlays", in: application)
                .isHittable,
            "the clear action must land in the open frame",
        )
        Thread.sleep(forTimeInterval: Self.frameSettleSeconds)
        captureScreenshot(named: Self.layersOpenFileName)
    }

    // MARK: The shared driving rules

    /// Launches a fresh application with the runtime credential in its launch
    /// environment (the documented automation seam) and a clean remembered
    /// choices record.
    private func launchApplication(
        launchArguments: [String],
        credential: String,
        providerKey: String?,
    ) -> XCUIApplication {
        let application = XCUIApplication()
        application.launchArguments = [
            "-DemoResetRememberedChoices", "YES",
        ] + launchArguments
        application.launchEnvironment[Self.credentialEnvironmentKey] = credential
        if let providerKey {
            application.launchEnvironment[Self.providerKeyEnvironmentKey] = providerKey
        }
        application.launch()
        return application
    }

    /// Awaits the readiness ladder's settled label for a keyed session.
    private func awaitReady(_ application: XCUIApplication) {
        let readinessChip = elementWithIdentifier("readiness-chip", in: application)
        XCTAssertTrue(
            readinessChip.waitForExistence(timeout: Self.firstFrameTimeoutSeconds),
            "the readiness chip must render",
        )
        let settledPredicate = NSPredicate(
            format: "label == %@", Self.readyChipLabel
        )
        let settledExpectation = expectation(
            for: settledPredicate, evaluatedWith: readinessChip
        )
        wait(for: [settledExpectation], timeout: Self.firstFrameTimeoutSeconds)
    }

    /// Drags the settings sheet's content upward by a fraction of its height
    /// (a full swipe overshoots the capability rows).
    private func dragSheetUp(byFraction fraction: CGFloat, in sheet: XCUIElement) {
        let start = sheet.coordinate(
            withNormalizedOffset: CGVector(dx: 0.5, dy: 0.78)
        )
        let end = sheet.coordinate(
            withNormalizedOffset: CGVector(dx: 0.5, dy: 0.78 - fraction)
        )
        // A slow drag with a hold at the end suppresses the fling momentum,
        // so the capability rows land where this fraction puts them.
        start.press(
            forDuration: 0.1,
            thenDragTo: end,
            withVelocity: XCUIGestureVelocity(100),
            thenHoldForDuration: 0.5
        )
    }

    /// One identifier's first matching element in the accessibility tree.
    private func elementWithIdentifier(
        _ identifier: String,
        in application: XCUIApplication,
    ) -> XCUIElement {
        application.descendants(matching: .any)
            .matching(identifier: identifier)
            .firstMatch
    }

    /// Keeps one full-screen PNG as a result-bundle attachment.
    private func captureScreenshot(named fileName: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = fileName
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
