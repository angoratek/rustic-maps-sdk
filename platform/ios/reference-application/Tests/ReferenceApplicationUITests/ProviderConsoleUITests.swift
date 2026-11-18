//! The provider console's XCUITest cases: the floating control opens and
//! closes the sheet, and the control carries the unread Warning/Error dot
//! when one arrived while the console was closed. Both cases drive the
//! application's own open path and the launch-automation probe seam; no case
//! reaches the network, supplies a provider credential or issues a provider
//! call.

import XCTest

/// The provider console's UI cases.
final class ProviderConsoleUITests: XCTestCase {
    private static let booleanTrueValue = "YES"
    private static let resetRememberedChoicesArgument =
        "-DemoResetRememberedChoices"
    private static let probeWarningArgument = "-DemoConsoleProbeWarning"

    private static let controlIdentifier = "provider-console-control"
    private static let unreadDotIdentifier = "provider-console-unread-dot"
    private static let sheetIdentifier = "provider-console"
    private static let closeIdentifier = "provider-console-close"

    private static let settleTimeoutSeconds: TimeInterval = 30

    /// The launch-environment keys the application reads for the keyless
    /// search and routing providers' deployment addresses. The cases here
    /// never search or route, so both point at a dead loopback address and no
    /// provider request leaves the simulator.
    private static let searchDeploymentBaseURLEnvironmentKey =
        "SEARCH_PROVIDER_BASE_URL"
    private static let routingDeploymentBaseURLEnvironmentKey =
        "ROUTING_PROVIDER_BASE_URL"
    private static let unreachableDeploymentBaseURL = "http://127.0.0.1:1"

    /// Launches the application keyless with the given extra arguments.
    private func launchApplication(
        extraArguments: [String] = []
    ) -> XCUIApplication {
        let application = XCUIApplication()
        application.launchArguments = [
            Self.resetRememberedChoicesArgument,
            Self.booleanTrueValue,
        ] + extraArguments
        application.launchEnvironment[
            Self.searchDeploymentBaseURLEnvironmentKey
        ] = Self.unreachableDeploymentBaseURL
        application.launchEnvironment[
            Self.routingDeploymentBaseURLEnvironmentKey
        ] = Self.unreachableDeploymentBaseURL
        application.launch()
        return application
    }

    /// The element carrying one accessibility identifier.
    private func element(
        _ identifier: String,
        in application: XCUIApplication
    ) -> XCUIElement {
        application.descendants(matching: .any)
            .matching(identifier: identifier)
            .firstMatch
    }

    /// Polls for the element to disappear.
    private func waitForNonExistence(
        _ element: XCUIElement,
        timeout: TimeInterval
    ) -> Bool {
        let nonExistenceExpectation = XCTNSPredicateExpectation(
            predicate: NSPredicate(format: "exists == false"),
            object: element
        )
        return XCTWaiter.wait(
            for: [nonExistenceExpectation],
            timeout: timeout
        ) == .completed
    }

    /// Polls for the control's unread dot to appear.
    private func waitForUnreadDot(
        in application: XCUIApplication,
        timeout: TimeInterval
    ) -> Bool {
        element(Self.unreadDotIdentifier, in: application)
            .waitForExistence(timeout: timeout)
    }

    func testTheConsoleOpensAndClosesFromItsFloatingControl() {
        let application = launchApplication()

        let control = element(Self.controlIdentifier, in: application)
        XCTAssertTrue(
            control.waitForExistence(
                timeout: Self.settleTimeoutSeconds
            ),
            "the floating console control must render on the map"
        )
        control.tap()

        let sheet = element(Self.sheetIdentifier, in: application)
        XCTAssertTrue(
            sheet.waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the floating control's tap must open the console sheet"
        )

        element(Self.closeIdentifier, in: application).tap()
        XCTAssertTrue(
            waitForNonExistence(
                sheet,
                timeout: Self.settleTimeoutSeconds
            ),
            "the console close control must dismiss the sheet"
        )
    }

    func testTheConsoleControlCarriesTheUnreadWarningDot() {
        let application = launchApplication(
            extraArguments: [
                Self.probeWarningArgument,
                Self.booleanTrueValue,
            ]
        )

        let control = element(Self.controlIdentifier, in: application)
        XCTAssertTrue(
            control.waitForExistence(
                timeout: Self.settleTimeoutSeconds
            ),
            "the floating console control must render on the map"
        )
        XCTAssertTrue(
            waitForUnreadDot(
                in: application,
                timeout: Self.settleTimeoutSeconds
            ),
            "a warning recorded while the console is closed must raise the unread dot"
        )

        control.tap()
        XCTAssertTrue(
            element(Self.sheetIdentifier, in: application)
                .waitForExistence(timeout: Self.settleTimeoutSeconds),
            "the dotted control still opens the console"
        )
    }
}
