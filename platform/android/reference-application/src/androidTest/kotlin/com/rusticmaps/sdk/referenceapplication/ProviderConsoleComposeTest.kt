package com.rusticmaps.sdk.referenceapplication

import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.rustic_maps.LogEvent
import uniffi.rustic_maps.SdkLogSeverity

/**
 * The developer console's Compose cases: the floating control opens and closes
 * the sheet, and the Warning/Error dot reports a warning published while the
 * console is closed. The injected event is a fake listener input, never a
 * provider call.
 */
@RunWith(AndroidJUnit4::class)
class ProviderConsoleComposeTest {
    @get:Rule
    val composeRule = createAndroidComposeRule<MainActivity>()

    private val applicationState: ReferenceApplicationState
        get() = requireNotNull(composeRule.activity.automationApplicationState) {
            "the application state is constructed on the first composition"
        }

    @Before
    fun awaitsTheConstructedApplicationState() {
        composeRule.waitUntil(timeoutMillis = APPLICATION_STATE_TIMEOUT_MILLISECONDS) {
            composeRule.activity.automationApplicationState != null
        }
    }

    @After
    fun closesTheConsoleAndClearsItsStream() {
        composeRule.runOnUiThread {
            composeRule.activity.automationApplicationState?.let { state ->
                state.dismissProviderConsole()
                state.clearDiagnosticsEvents()
            }
        }
        composeRule.waitForIdle()
    }

    @Test
    fun theFloatingControlOpensAndClosesTheConsole() {
        composeRule.onNodeWithTag(
            ReferenceApplicationConstants.DEVELOPER_CONSOLE_CONTROL_TEST_TAG,
        ).assertExists()

        composeRule.onNodeWithTag(
            ReferenceApplicationConstants.DEVELOPER_CONSOLE_CONTROL_TEST_TAG,
        ).performClick()
        composeRule.onNodeWithTag(
            ReferenceApplicationConstants.DEVELOPER_CONSOLE_SHEET_TEST_TAG,
        ).assertExists()

        composeRule.onNodeWithTag(
            ReferenceApplicationConstants.DEVELOPER_CONSOLE_CLOSE_TEST_TAG,
        ).performClick()
        composeRule.onNodeWithTag(
            ReferenceApplicationConstants.DEVELOPER_CONSOLE_SHEET_TEST_TAG,
        ).assertDoesNotExist()
    }

    @Test
    fun theDotReportsAWarningThatArrivesWhileTheConsoleIsClosed() {
        // The first open engages the shared opt-in stream; close it so a later
        // warning is an unseen event.
        composeRule.runOnUiThread { applicationState.openProviderConsole() }
        composeRule.waitForIdle()
        composeRule.runOnUiThread { applicationState.dismissProviderConsole() }
        composeRule.waitForIdle()
        composeRule.onNodeWithTag(
            ReferenceApplicationConstants.DEVELOPER_CONSOLE_WARNING_DOT_TEST_TAG,
        ).assertDoesNotExist()

        composeRule.runOnUiThread {
            applicationState.diagnosticEventListener.onDiagnosticEvent(
                LogEvent(
                    timestampMillisecondsSinceEpoch = 1UL,
                    severity = SdkLogSeverity.WARN,
                    subsystemTarget = "tile_fetch",
                    messageLine = "a warning arrived while the console was closed",
                    providerRequestTrace = null,
                ),
            )
            applicationState.diagnosticEventListener.publishPendingEvents()
        }
        composeRule.waitForIdle()
        composeRule.onNodeWithTag(
            ReferenceApplicationConstants.DEVELOPER_CONSOLE_WARNING_DOT_TEST_TAG,
        ).assertExists()

        composeRule.runOnUiThread { applicationState.openProviderConsole() }
        composeRule.waitForIdle()
        composeRule.runOnUiThread { applicationState.dismissProviderConsole() }
        composeRule.waitForIdle()
        composeRule.onNodeWithTag(
            ReferenceApplicationConstants.DEVELOPER_CONSOLE_WARNING_DOT_TEST_TAG,
        ).assertDoesNotExist()
    }

    private companion object {
        /** How long the suite waits for the first composition to build the state. */
        const val APPLICATION_STATE_TIMEOUT_MILLISECONDS: Long = 20_000L
    }
}
