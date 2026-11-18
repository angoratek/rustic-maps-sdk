package com.rusticmaps.sdk.referenceapplication

import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertHeightIsEqualTo
import androidx.compose.ui.test.assertWidthIsEqualTo
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.rusticmaps.sdk.ux.ReferenceAppUXDimensions
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The Compose-semantics half of the connected suite (M67): the reference
 * application exposes one surface for the map chrome.
 *
 * The settings sheet renders no base-map, style or overlay-scene section;
 * the floating layers control owns the base maps and the scene switches and
 * offers the clear action; every map control uses the shared
 * `map-control-diameter`; and the drawer carries no demo-plan or demo-scene
 * picker. Every assertion reads the composed semantics tree, so a section
 * that only moved in state but still renders fails here.
 *
 * The class launches the real activity through the Compose rule and drives
 * the controls by their semantics, never by a mirrored state value.
 */
@RunWith(AndroidJUnit4::class)
class ReferenceApplicationComposeConvergenceTest {

    /** The activity launch plus the Compose semantics host for every case. */
    @get:Rule
    val composeRule = createAndroidComposeRule<MainActivity>()

    /** The one diameter every map control must use (the shared token). */
    private val mapControlDiameter = ReferenceAppUXDimensions.MapControlDiameter

    @Test
    fun theSettingsSheetCarriesNoBaseMapStyleOrSceneSection() {
        openSettings()

        // The settings surface's two provider sections remain: the descriptor
        // credential rows and the SDK-readback capability rows. The row name
        // comes from the shared registry, never a shell literal.
        val credentialRowName = composeRule.runOnIdle {
            applicationState().providerCredentialDescriptors.first().displayName
        }
        composeRule.onNodeWithText(credentialRowName).assertExists()
        composeRule.onNodeWithText(ReferenceApplicationConstants.CAPABILITY_SECTION_LABEL_TEXT)
            .assertExists()
        composeRule.onNodeWithTag(ReferenceApplicationConstants.CAPABILITY_ROUTING_ROW_TEST_TAG)
            .assertExists()

        // The old homes are gone: no base-map section, no vector-style group
        // and no overlay-scene switches or pickers.
        composeRule.onNodeWithText(ReferenceApplicationConstants.BASE_MAP_SECTION_LABEL_TEXT)
            .assertDoesNotExist()
        composeRule.onNodeWithText(ReferenceApplicationConstants.BASE_MAP_VECTOR_GROUP_LABEL_TEXT)
            .assertDoesNotExist()
        composeRule.onNodeWithText(ReferenceApplicationConstants.MAP_LAYERS_OVERLAYS_SECTION_LABEL_TEXT)
            .assertDoesNotExist()
        composeRule.onNodeWithText(ReferenceApplicationConstants.DEMO_SCENES_ROW_TITLE_TEXT)
            .assertDoesNotExist()
        composeRule.onNodeWithText(ReferenceApplicationConstants.DEMO_PLANS_ROW_TITLE_TEXT)
            .assertDoesNotExist()

        // The accepted settings surfaces stay where they were: the working
        // sheet keeps Offline maps and Theme, and the readiness chip still
        // opens Diagnostics. None of them is a base-map, style or scene home.
        composeRule.onNodeWithTag(ReferenceApplicationConstants.SETTINGS_CLOSE_TEST_TAG)
            .performClick()
        composeRule.runOnIdle {
            applicationState().requestHomeSheetState(
                ReferenceApplicationState.HomeSheetState.HALF,
            )
        }
        composeRule.onNodeWithText(ReferenceApplicationConstants.OFFLINE_MAPS_CARD_TITLE_TEXT)
            .assertExists()
        composeRule.onNodeWithText(ReferenceApplicationConstants.THEME_ROW_TITLE_TEXT)
            .assertExists()
        composeRule.onNodeWithTag(ReferenceApplicationConstants.READINESS_CHIP_TEST_TAG)
            .performClick()
        composeRule.onNodeWithText(ReferenceApplicationConstants.DIAGNOSTICS_TITLE_TEXT)
            .assertExists()
    }

    @Test
    fun theLayersControlOwnsTheBaseMapsAndScenesAndOffersTheClearAction() {
        waitForApplicationState()
        composeRule.onNodeWithTag(ReferenceApplicationConstants.MAP_LAYERS_CONTROL_TEST_TAG)
            .performClick()
        composeRule.onNodeWithTag(ReferenceApplicationConstants.MAP_LAYERS_PANEL_TEST_TAG)
            .assertExists()

        // Base maps and scenes both live here, not in the settings sheet.
        composeRule.onNodeWithText(ReferenceApplicationConstants.BASE_MAP_SECTION_LABEL_TEXT)
            .assertExists()
        composeRule.onNodeWithText(ReferenceApplicationConstants.MAP_LAYERS_OVERLAYS_SECTION_LABEL_TEXT)
            .assertExists()
        val clearAction = composeRule.onNodeWithText(
            ReferenceApplicationConstants.OVERLAYS_CLEAR_ACTION_TEXT,
        )
        clearAction.assertExists().assertHasClickAction()

        // The rows come from the shared registry and scenes artifacts, so the
        // case reads the runtime identifiers instead of naming a provider or
        // a scene.
        val baseRowIdentifier = composeRule.runOnIdle {
            applicationState().mapLayersState.baseRows.first().identifier
        }
        composeRule.onNodeWithTag(baseRowIdentifier).assertExists()
        val sceneKey = composeRule.runOnIdle {
            applicationState().artifact.overlayScenes.rows.first().sceneKey
        }
        val sceneSwitchTag =
            ReferenceApplicationConstants.MAP_LAYER_SCENE_ROW_TEST_TAG_PREFIX + sceneKey
        val sceneSwitch = composeRule.onNodeWithTag(sceneSwitchTag)
        sceneSwitch.performScrollTo().assertExists()
        sceneSwitch.performClick()
        composeRule.runOnIdle {
            assertTrue(
                "a scene switch in the layers panel must record its choice",
                applicationState().sceneToggleStates[sceneKey] == true,
            )
        }

        // The clear action clears every overlay scene from the same panel.
        clearAction.performScrollTo().performClick()
        composeRule.runOnIdle {
            assertFalse(
                "Clear overlays must clear every scene switch",
                applicationState().sceneToggleStates[sceneKey] == true,
            )
        }
    }

    @Test
    fun everyMapControlUsesTheSharedFortyUnitDiameter() {
        waitForApplicationState()
        assertControlDiameter(ReferenceApplicationConstants.MAP_CONTROL_ZOOM_IN_ACCESSIBILITY_TEXT)
        assertControlDiameter(ReferenceApplicationConstants.MAP_CONTROL_ZOOM_OUT_ACCESSIBILITY_TEXT)
        assertControlDiameter(ReferenceApplicationConstants.MAP_CONTROL_MY_LOCATION_ACCESSIBILITY_TEXT)
        assertControlDiameter(ReferenceApplicationConstants.MAP_CONTROL_PLANE_ACCESSIBILITY_TEXT)
        assertControlDiameter(ReferenceApplicationConstants.ATTRIBUTION_INFO_ACCESSIBILITY_TEXT)

        // The layers control is the one map control identified by a test tag.
        composeRule.onNodeWithTag(ReferenceApplicationConstants.MAP_LAYERS_CONTROL_TEST_TAG)
            .assertWidthIsEqualTo(mapControlDiameter)
            .assertHeightIsEqualTo(mapControlDiameter)
    }

    @Test
    fun theDrawerListsNoDemoPlanOrDemoScenePickers() {
        waitForApplicationState()
        // The peek's search field opens the drawer's search view, the surface
        // that used to carry the demo-plan chips.
        composeRule.onNodeWithTag(ReferenceApplicationConstants.HOME_SHEET_PEEK_SEARCH_TEST_TAG)
            .performClick()
        composeRule.onNodeWithText(ReferenceApplicationConstants.PLAN_SEARCH_HEADING_TEXT)
            .assertExists()

        // The demo content is still driven by the shared artifact, but its
        // pickers are no longer visible rows in the drawer.
        composeRule.onNodeWithText(ReferenceApplicationConstants.DEMO_PLANS_ROW_TITLE_TEXT)
            .assertDoesNotExist()
        composeRule.onNodeWithText(ReferenceApplicationConstants.DEMO_SCENES_ROW_TITLE_TEXT)
            .assertDoesNotExist()
    }

    /** Opens the settings sheet through the readiness dock's gear control. */
    private fun openSettings() {
        waitForApplicationState()
        composeRule.onNodeWithTag(ReferenceApplicationConstants.SETTINGS_OPEN_TEST_TAG)
            .performClick()
        composeRule.onNodeWithTag(ReferenceApplicationConstants.SETTINGS_CLOSE_TEST_TAG)
            .assertExists()
    }

    /**
     * Waits for the activity's composition to build the application state.
     * The gear is the dock's first state-dependent control, so its presence
     * proves the state exists before a case drives any other control.
     */
    private fun waitForApplicationState() {
        composeRule.waitUntil(timeoutMillis = APPLICATION_STATE_TIMEOUT_MILLISECONDS) {
            composeRule.activity.automationApplicationState != null &&
                composeRule.onAllNodesWithTag(
                    ReferenceApplicationConstants.SETTINGS_OPEN_TEST_TAG,
                ).fetchSemanticsNodes().isNotEmpty()
        }
    }

    /** Asserts one circular control's semantics node carries the shared diameter. */
    private fun assertControlDiameter(contentDescription: String) {
        composeRule.onNodeWithContentDescription(contentDescription)
            .assertWidthIsEqualTo(mapControlDiameter)
            .assertHeightIsEqualTo(mapControlDiameter)
    }

    /** The constructed application state the activity exposes to the suite. */
    private fun applicationState(): ReferenceApplicationState =
        composeRule.activity.automationApplicationState
            ?: throw AssertionError("the reference application constructed no state")

    private companion object {
        /** How long the composition may take to build the application state. */
        const val APPLICATION_STATE_TIMEOUT_MILLISECONDS = 20_000L
    }
}
