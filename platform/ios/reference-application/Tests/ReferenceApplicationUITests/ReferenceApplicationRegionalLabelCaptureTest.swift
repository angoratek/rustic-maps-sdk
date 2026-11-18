//! The opt-in capture of the iOS reference application's dense regional label
//! scene: the keyless OpenFreeMap Bright vector base, one zoom step wider than
//! the launch camera, with its place names and road names placed by the shared
//! label pass.
//!
//! The capture is opt-in and keyless; the vector base is a registry descriptor
//! and registers no provider credential:
//!
//! ```
//! TEST_RUNNER_CAPTURE_REGIONAL_LABELS=1 \
//! xcodebuild test -project ReferenceApplication.xcodeproj \
//!   -scheme ReferenceApplication -sdk iphonesimulator \
//!   -destination 'platform=iOS Simulator,name=iPhone 17 Pro' \
//!   -only-testing:ReferenceApplicationUITests/ReferenceApplicationRegionalLabelCaptureTest \
//!   -resultBundlePath tmp/reference-screenshots/ios/ReferenceApplicationRegionalLabels.xcresult
//! ```
//!
//! The application zooms out through its own control method and selects the
//! first vector style through the layers panel's own row action; no camera or
//! label state is forced. The screenshot is exported from the result bundle's
//! attachments.

import XCTest

/// The dense regional label capture from one keyless run.
final class ReferenceApplicationRegionalLabelCaptureTest: XCTestCase {
    /// The opt-in environment flag the runner reads.
    private static let captureEnvironmentKey = "CAPTURE_REGIONAL_LABELS"

    /// The launch argument that runs the zoom-out control's own method once,
    /// taking the launch camera from zoom 14 to the regional zoom 13.
    private static let zoomOutDemoArgument = "-DemoTapZoomOut"
    private static let resetRememberedChoicesArgument = "-DemoResetRememberedChoices"

    /// The readiness chip's existence proves the application attached its map
    /// view; a keyless session settles on the neutral keyless state.
    private static let readinessChipIdentifier = "readiness-chip"

    /// The layers control and its first committed vector style row.
    private static let layersControlIdentifier = "map-layers-control"
    private static let vectorStyleRowPrefix = "map-layer-style-"
    private static let layersCloseIdentifier = "map-layers-close"

    /// The capture file name the result bundle carries.
    private static let regionalLabelsFileName = "ios-regional-labels"

    /// A fresh session's first drawn frames on the simulator.
    private static let firstFrameTimeoutSeconds: TimeInterval = 120

    /// The layers panel's row interaction settles within this bound.
    private static let settleTimeoutSeconds: TimeInterval = 15

    /// The vector base's first labelled frame after the zoom step.
    private static let vectorSettleSeconds: TimeInterval = 12

    func testCapturesTheDenseRegionalLabelScene() throws {
        try XCTSkipUnless(
            ProcessInfo.processInfo.environment[Self.captureEnvironmentKey] == "1",
            "the regional label capture is opt-in",
        )
        let application = XCUIApplication()
        application.launchArguments = [
            Self.resetRememberedChoicesArgument, "YES",
            Self.zoomOutDemoArgument, "YES",
        ]
        application.launch()

        XCTAssertTrue(
            elementWithIdentifier(Self.readinessChipIdentifier, in: application)
                .waitForExistence(timeout: Self.firstFrameTimeoutSeconds),
            "the readiness chip must render",
        )
        let layersControl = elementWithIdentifier(
            Self.layersControlIdentifier, in: application
        )
        XCTAssertTrue(
            layersControl.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the layers control must render",
        )
        layersControl.tap()

        let vectorStyleRows = application.descendants(matching: .any)
            .matching(
                NSPredicate(
                    format: "identifier BEGINSWITH %@", Self.vectorStyleRowPrefix
                )
            )
        XCTAssertTrue(
            vectorStyleRows.firstMatch.waitForExistence(
                timeout: Self.settleTimeoutSeconds
            ),
            "the layers panel must offer a committed vector style row",
        )
        vectorStyleRows.firstMatch.tap()

        // The tapped row's Serving mark proves the styled vector base replaced
        // the keyless raster source before the capture.
        let servingPredicate = NSPredicate(format: "label CONTAINS %@", "Serving")
        let servingExpectation = expectation(
            for: servingPredicate, evaluatedWith: vectorStyleRows.firstMatch
        )
        wait(for: [servingExpectation], timeout: Self.settleTimeoutSeconds)

        elementWithIdentifier(Self.layersCloseIdentifier, in: application).tap()
        Thread.sleep(forTimeInterval: Self.vectorSettleSeconds)

        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = Self.regionalLabelsFileName
        attachment.lifetime = .keepAlways
        add(attachment)
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
}
