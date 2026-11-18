//! The SwiftUI reference application entry point: the map fills the
//! viewport with the floating panels above it (the layout archetype,
//! docs/ReferenceAppUX.md §1), opening on the Auto theme choice like every
//! reference-application session that never chose (§3).

import SwiftUI

@main
struct ReferenceApplicationApp: App {
    /// The application model the panels and the map surface render from.
    @StateObject private var applicationModel = ReferenceApplicationModel()

    /// The scene's lifecycle phase; leaving the active state flushes a
    /// gesture-settled choice record that has not been written yet.
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            // The root view resolves the theme choice (§3) against the
            // operating system's color scheme and applies the resolved
            // `appTheme`/`colorScheme` pair to its own subtree, so the
            // system preference stays readable here for Auto.
            ReferenceApplicationView(applicationModel: applicationModel)
                .onChange(of: scenePhase) { phase in
                    guard phase != .active else { return }
                    applicationModel.flushPendingChoicePersist()
                }
        }
    }
}