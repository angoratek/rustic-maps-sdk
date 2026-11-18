//! The type-ahead UI cases (`docs/design/Autocomplete.md`): the three-
//! character threshold, the quiet window, the five-row cap, cancellation of
//! a superseded request, selection filling the field and planning, the
//! no-match and failure states, and the keyless provider the startup
//! installs from the shared registry.
//!
//! Every suggestion comes from the committed fixture documents through a
//! loopback fixture server the application reaches through the launch-
//! environment deployment override; no test touches the network.

import Foundation
import XCTest

final class SearchTypeAheadUITests: XCTestCase {
    // MARK: Named values

    /// The launch argument that opens the search view at startup.
    private static let launchArgumentSearchOpen = "DemoOpenSearch"
    /// The launch argument that clears the remembered choices at startup.
    private static let launchArgumentResetRememberedChoices = "DemoResetRememberedChoices"
    /// The launch argument domain's true value.
    private static let launchArgumentTrueValue = "YES"
    /// The launch-environment key the application reads for the keyless
    /// search provider's deployment address.
    private static let deploymentBaseURLEnvironmentKey = "SEARCH_PROVIDER_BASE_URL"
    /// The destination field's placeholder (its accessibility lookup).
    private static let destinationFieldPlaceholder = "Search a destination"
    /// The origin field's placeholder.
    private static let originFieldPlaceholder = "Search an origin"
    /// A field's suggestion-row identifier prefix.
    private static func suggestionRowIdentifierPrefix(forField field: String) -> String {
        "\(field)-suggestion-row-"
    }
    /// The no-match row's identifier.
    private static func noMatchesIdentifier(forField field: String) -> String {
        "\(field)-suggestion-no-matches"
    }
    /// The one inline failure sentence's identifier.
    private static func failureIdentifier(forField field: String) -> String {
        "\(field)-suggestion-error"
    }
    /// The search view's explicit Plan route action.
    private static let planRouteButtonIdentifier = "plan-route-button"
    /// The route summary card the plan presents.
    private static let routeSummaryIdentifier = "route-summary-card"
    /// The recorded demo route's summary line.
    private static let recordedRouteSummaryText =
        "Recorded demo route, pedestrian, fastest."
    /// The keyboard dismissal control.
    private static let keyboardDismissIdentifier = "search-keyboard-dismiss"

    /// The application's first interactive state's bound (the simulator's
    /// graphics stack settles well inside it).
    private static let firstInteractiveTimeoutSeconds: TimeInterval = 120
    /// One suggestion list's bound after a keystroke.
    private static let suggestionTimeoutSeconds: TimeInterval = 10
    /// The quiet window the threshold case waits out (longer than the
    /// documented 250 ms).
    private static let quietWindowObservationSeconds: TimeInterval = 0.9
    /// The superseded answer's delay: it must arrive after the newer query's
    /// answer, so the list proves the drop.
    private static let supersededAnswerDelaySeconds: TimeInterval = 1.5
    /// How long the supersede case waits past that delay.
    private static let supersededAnswerObservationSeconds: TimeInterval = 1.8
    /// The poll cadence of the row-count cases.
    private static let pollIntervalSeconds: TimeInterval = 0.05

    /// The application under test.
    private var application: XCUIApplication?

    // MARK: Lifecycle

    override func setUpWithError() throws {
        try super.setUpWithError()
        continueAfterFailure = false
    }

    override func tearDown() {
        application?.terminate()
        application = nil
        super.tearDown()
    }

    // MARK: The rule

    /// Two typed characters never ask; the third asks once, after the quiet
    /// window, and the keyless provider installed from the registry answers.
    func testTheThreeCharacterThresholdAndTheQuietWindow() throws {
        let defaultBody = try fixtureData(named: "type_ahead_default")
        let server = try FixtureSearchServer { _, _ in
            FixtureSearchResponse(
                statusCode: 200,
                body: defaultBody,
                delaySeconds: 0
            )
        }
        let port = try server.start()
        defer { server.stop() }
        let application = launchApplication(
            deploymentBaseURL: "http://127.0.0.1:\(port)"
        )
        let destinationField = waitForField(
            Self.destinationFieldPlaceholder,
            in: application
        )

        destinationField.tap()
        destinationField.typeText("Ca")
        Thread.sleep(forTimeInterval: Self.quietWindowObservationSeconds)

        XCTAssertEqual(
            server.requests().count, 0,
            "a query below the three-character threshold must not ask the provider"
        )
        XCTAssertEqual(
            suggestionRows(forField: "destination", in: application).count, 0,
            "a query below the threshold shows nothing"
        )

        destinationField.typeText("p")
        XCTAssertTrue(
            waitForSuggestionRow(
                "Capitol Park", forField: "destination", in: application
            ),
            "the third character must reveal the provider's suggestions"
        )
        let requests = server.requests()
        XCTAssertEqual(
            requests.count, 1,
            "the debounce must coalesce the keystrokes into one request"
        )
        XCTAssertEqual(requests.first?.queryText, "Cap")
    }

    /// The provider returns seven candidates and the field shows five; the
    /// provider asked for the same shared cap.
    func testTheFiveRowCapAndTheProvidersRequestedLimit() throws {
        let sevenRowsBody = try fixtureData(named: "type_ahead_seven_rows")
        let server = try FixtureSearchServer { _, _ in
            FixtureSearchResponse(
                statusCode: 200,
                body: sevenRowsBody,
                delaySeconds: 0
            )
        }
        let port = try server.start()
        defer { server.stop() }
        let application = launchApplication(
            deploymentBaseURL: "http://127.0.0.1:\(port)"
        )
        let destinationField = waitForField(
            Self.destinationFieldPlaceholder,
            in: application
        )

        destinationField.tap()
        destinationField.typeText("Seven")

        XCTAssertTrue(
            waitForSuggestionRowCount(
                5, forField: "destination", in: application
            ),
            "the list must stop at the shared five-row cap"
        )
        XCTAssertEqual(
            suggestionRows(forField: "destination", in: application).count, 5
        )
        XCTAssertEqual(
            server.requests().last?.resultLimit, "5",
            "the provider must be asked for the same five candidates"
        )
    }

    /// A superseded request's late answer never replaces the newer list.
    func testASupersededRequestNeverReplacesTheList() throws {
        let defaultBody = try fixtureData(named: "type_ahead_default")
        let alternateBody = try fixtureData(named: "type_ahead_alternate")
        let server = try FixtureSearchServer { _, queryItems in
            if queryItems["q"] == "Cap" {
                return FixtureSearchResponse(
                    statusCode: 200,
                    body: defaultBody,
                    delaySeconds: Self.supersededAnswerDelaySeconds
                )
            }
            return FixtureSearchResponse(
                statusCode: 200,
                body: alternateBody,
                delaySeconds: 0
            )
        }
        let port = try server.start()
        defer { server.stop() }
        let application = launchApplication(
            deploymentBaseURL: "http://127.0.0.1:\(port)"
        )
        let destinationField = waitForField(
            Self.destinationFieldPlaceholder,
            in: application
        )

        destinationField.tap()
        destinationField.typeText("Cap")
        XCTAssertTrue(
            server.waitForRequest(queryText: "Cap"),
            "the first query must reach the provider before it is superseded"
        )

        // Replace the text; every keystroke supersedes the older request.
        destinationField.typeText(
            String(
                repeating: XCUIKeyboardKey.delete.rawValue,
                count: "Cap".count
            ) + "Tow"
        )
        XCTAssertTrue(
            waitForSuggestionRow(
                "Old Sacramento", forField: "destination", in: application
            ),
            "the current query's answer must fill the list"
        )

        Thread.sleep(forTimeInterval: Self.supersededAnswerObservationSeconds)

        XCTAssertFalse(
            suggestionRowExists(
                "Capitol Park", forField: "destination", in: application
            ),
            "the superseded answer must never replace the newer list"
        )
        XCTAssertTrue(
            suggestionRowExists(
                "Old Sacramento", forField: "destination", in: application
            ),
            "the current query's answer must stay"
        )
        let askedQueries = server.requests().map(\.queryText)
        XCTAssertTrue(askedQueries.contains("Cap"))
        XCTAssertTrue(askedQueries.contains("Tow"))
    }

    /// Choosing a suggestion fills the field, closes the list, and the
    /// explicit Plan route action resolves the pair exactly as typed text.
    func testSelectionFillsTheFieldAndPlansExactlyAsTypedText() throws {
        let defaultBody = try fixtureData(named: "type_ahead_default")
        let server = try FixtureSearchServer { _, _ in
            FixtureSearchResponse(
                statusCode: 200,
                body: defaultBody,
                delaySeconds: 0
            )
        }
        let port = try server.start()
        defer { server.stop() }
        let application = launchApplication(
            deploymentBaseURL: "http://127.0.0.1:\(port)"
        )
        let originField = waitForField(
            Self.originFieldPlaceholder,
            in: application
        )

        originField.tap()
        originField.typeText("Cap")
        if waitForSuggestionRow(
            "Capitol Park", forField: "origin", in: application
        ) == false {
            return XCTFail("the origin field must offer the recorded start")
        }
        suggestionRow(
            "Capitol Park", forField: "origin", in: application
        ).tap()

        XCTAssertTrue(
            waitForFieldValue("Capitol Park", field: originField),
            "a chosen suggestion fills the field with its display line"
        )
        XCTAssertTrue(
            waitForSuggestionRowCount(0, forField: "origin", in: application),
            "choosing a suggestion closes its list"
        )

        let destinationField = waitForField(
            Self.destinationFieldPlaceholder,
            in: application
        )
        destinationField.tap()
        destinationField.typeText("Tow")
        if waitForSuggestionRow(
            "Tower Bridge", forField: "destination", in: application
        ) == false {
            return XCTFail("the destination field must offer the recorded end")
        }
        suggestionRow(
            "Tower Bridge", forField: "destination", in: application
        ).tap()
        XCTAssertTrue(
            waitForFieldValue("Tower Bridge", field: destinationField),
            "a chosen suggestion fills the destination field"
        )

        // The explicit Plan route action resolves the selected pair through
        // the same path typed text takes.
        if element(
            Self.keyboardDismissIdentifier, in: application
        ).exists {
            element(Self.keyboardDismissIdentifier, in: application).tap()
        }
        let planRouteButton = element(
            Self.planRouteButtonIdentifier, in: application
        )
        XCTAssertTrue(
            planRouteButton.waitForExistence(
                timeout: Self.suggestionTimeoutSeconds
            )
        )
        planRouteButton.tap()
        XCTAssertTrue(
            element(Self.routeSummaryIdentifier, in: application)
                .waitForExistence(timeout: Self.suggestionTimeoutSeconds),
            "the selected pair must plan exactly as typed text"
        )
        XCTAssertTrue(
            application.staticTexts[Self.recordedRouteSummaryText].exists,
            "the keyless plan must resolve the recorded route for the selected pair"
        )
    }

    /// A provider failure keeps the field's text and shows one inline
    /// sentence; a later keystroke retries and clears it.
    func testAProviderFailureKeepsTheTextAndALaterKeystrokeRetries() throws {
        let defaultBody = try fixtureData(named: "type_ahead_default")
        let server = try FixtureSearchServer { _, queryItems in
            if queryItems["q"] == "Fail" {
                return FixtureSearchResponse(
                    statusCode: 500,
                    body: Data("{\"error\":\"unavailable\"}".utf8),
                    delaySeconds: 0
                )
            }
            return FixtureSearchResponse(
                statusCode: 200,
                body: defaultBody,
                delaySeconds: 0
            )
        }
        let port = try server.start()
        defer { server.stop() }
        let application = launchApplication(
            deploymentBaseURL: "http://127.0.0.1:\(port)"
        )
        let destinationField = waitForField(
            Self.destinationFieldPlaceholder,
            in: application
        )

        destinationField.tap()
        destinationField.typeText("Fail")

        let failureLine = element(
            Self.failureIdentifier(forField: "destination"), in: application
        )
        XCTAssertTrue(
            failureLine.waitForExistence(
                timeout: Self.suggestionTimeoutSeconds
            ),
            "a provider failure must show its one inline sentence"
        )
        XCTAssertTrue(
            failureLine.label.contains("Suggestions are unavailable"),
            "got '\(failureLine.label)'"
        )
        XCTAssertEqual(
            destinationField.value as? String, "Fail",
            "a failure must keep the field's text"
        )
        XCTAssertEqual(
            suggestionRows(forField: "destination", in: application).count, 0,
            "a failure shows no rows"
        )

        destinationField.typeText("1")
        XCTAssertTrue(
            waitForSuggestionRow(
                "Capitol Park", forField: "destination", in: application
            ),
            "a later keystroke must retry the provider"
        )
        XCTAssertFalse(
            failureLine.exists,
            "a successful retry clears the inline sentence"
        )
    }

    /// A query the provider answers with no candidates shows one row.
    func testNoMatchesShowsOneRow() throws {
        let emptyBody = try fixtureData(named: "type_ahead_empty")
        let server = try FixtureSearchServer { _, _ in
            FixtureSearchResponse(
                statusCode: 200,
                body: emptyBody,
                delaySeconds: 0
            )
        }
        let port = try server.start()
        defer { server.stop() }
        let application = launchApplication(
            deploymentBaseURL: "http://127.0.0.1:\(port)"
        )
        let destinationField = waitForField(
            Self.destinationFieldPlaceholder,
            in: application
        )

        destinationField.tap()
        destinationField.typeText("Nope")

        XCTAssertTrue(
            element(
                Self.noMatchesIdentifier(forField: "destination"),
                in: application
            )
            .waitForExistence(timeout: Self.suggestionTimeoutSeconds),
            "no candidates must show the one no-match row"
        )
        XCTAssertEqual(
            suggestionRows(forField: "destination", in: application).count, 0,
            "the no-match state is a row, never a suggestion list"
        )
    }

    // MARK: Helpers

    /// Launches the application with the search view open and the keyless
    /// search provider pointed at the fixture server.
    private func launchApplication(
        deploymentBaseURL: String
    ) -> XCUIApplication {
        let application = XCUIApplication()
        application.launchArguments = [
            "-\(Self.launchArgumentResetRememberedChoices)",
            Self.launchArgumentTrueValue,
            "-\(Self.launchArgumentSearchOpen)",
            Self.launchArgumentTrueValue,
        ]
        application.launchEnvironment[
            Self.deploymentBaseURLEnvironmentKey
        ] = deploymentBaseURL
        // These cases exercise the keyless path; an inherited automation
        // credential must not switch the provider to the keyed one.
        application.launchEnvironment["PROVIDER_API_KEY"] = ""
        application.launchEnvironment["PROVIDER_KEY"] = ""
        application.launch()
        self.application = application
        return application
    }

    /// Waits for one search field by its placeholder.
    private func waitForField(
        _ placeholder: String,
        in application: XCUIApplication
    ) -> XCUIElement {
        let field = application.textFields[placeholder]
        XCTAssertTrue(
            field.waitForExistence(
                timeout: Self.firstInteractiveTimeoutSeconds
            ),
            "the search view must render its '\(placeholder)' field"
        )
        return field
    }

    /// One field's suggestion rows.
    private func suggestionRows(
        forField field: String,
        in application: XCUIApplication
    ) -> XCUIElementQuery {
        application.buttons.matching(
            NSPredicate(
                format: "identifier BEGINSWITH %@",
                Self.suggestionRowIdentifierPrefix(forField: field)
            )
        )
    }

    /// One suggestion row by its display line.
    private func suggestionRow(
        _ displayText: String,
        forField field: String,
        in application: XCUIApplication
    ) -> XCUIElement {
        suggestionRows(forField: field, in: application)
            .matching(
                NSPredicate(format: "label == %@", displayText)
            )
            .firstMatch
    }

    /// Whether a named suggestion row is present.
    private func suggestionRowExists(
        _ displayText: String,
        forField field: String,
        in application: XCUIApplication
    ) -> Bool {
        suggestionRow(displayText, forField: field, in: application).exists
    }

    /// Waits for a named suggestion row.
    private func waitForSuggestionRow(
        _ displayText: String,
        forField field: String,
        in application: XCUIApplication
    ) -> Bool {
        suggestionRow(
            displayText, forField: field, in: application
        ).waitForExistence(timeout: Self.suggestionTimeoutSeconds)
    }

    /// Waits for exactly the given number of suggestion rows.
    private func waitForSuggestionRowCount(
        _ expectedCount: Int,
        forField field: String,
        in application: XCUIApplication
    ) -> Bool {
        let deadline = Date().addingTimeInterval(
            Self.suggestionTimeoutSeconds
        )
        while Date() < deadline {
            if suggestionRows(forField: field, in: application).count
                == expectedCount
            {
                return true
            }
            Thread.sleep(forTimeInterval: Self.pollIntervalSeconds)
        }
        return suggestionRows(forField: field, in: application).count
            == expectedCount
    }

    /// Waits for a field's rendered value to become the expected text.
    private func waitForFieldValue(
        _ expectedValue: String,
        field: XCUIElement
    ) -> Bool {
        let deadline = Date().addingTimeInterval(
            Self.suggestionTimeoutSeconds
        )
        while Date() < deadline {
            if (field.value as? String) == expectedValue {
                return true
            }
            Thread.sleep(forTimeInterval: Self.pollIntervalSeconds)
        }
        return (field.value as? String) == expectedValue
    }

    /// Any element carrying the given accessibility identifier.
    private func element(
        _ identifier: String,
        in application: XCUIApplication
    ) -> XCUIElement {
        application.descendants(matching: .any)
            .matching(identifier: identifier)
            .firstMatch
    }

    /// One committed fixture document from the test bundle.
    private func fixtureData(named documentName: String) throws -> Data {
        let bundle = Bundle(for: Self.self)
        guard let fixtureURL = bundle.url(
            forResource: documentName,
            withExtension: "json"
        ) else {
            throw FixtureSearchServerError.fixtureMissing(
                documentName: documentName
            )
        }
        return try Data(contentsOf: fixtureURL)
    }
}
