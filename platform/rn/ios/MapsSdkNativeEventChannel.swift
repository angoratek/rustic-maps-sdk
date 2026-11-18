import Foundation

/// The package's single native-to-JavaScript view event channel.
///
/// The native module installs the sink (its `RCTEventEmitter`); every host
/// view emits one named event carrying its JavaScript-side identifier, the
/// event name and the event's own fields, so the component can filter events
/// for its own view.
@objc(MapsSdkNativeEventChannel)
public final class MapsSdkNativeEventChannel: NSObject {

    /// The named event the JavaScript component subscribes to.
    @objc public static let eventName = "MapsSdkMapViewEvent"

    /// The named event the typed SDK proxy subscribes to for region progress.
    @objc public static let offlineRegionEventName = "MapsSdkOfflineRegionEvent"

    /// The module's emitter, or nil before the module initialized.
    @objc public static var sink: ((String, [String: Any]) -> Void)?

    /// Emits one view event with the identifier and name folded in.
    @objc public static func emit(_ payload: [String: Any]) {
        sink?(eventName, payload)
    }

    /// Emits one region-download progress report.
    @objc public static func emitOfflineRegionProgress(_ payload: [String: Any]) {
        sink?(offlineRegionEventName, payload)
    }
}
