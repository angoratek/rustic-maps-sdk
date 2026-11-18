//! The shared provider registry as the language-neutral descriptor source
//! every reference application loads: the settings surface renders one row per
//! descriptor from here, so a new provider is a registry entry plus a
//! registration dispatch arm, never a view rewrite. Descriptors carry labels,
//! availability, attribution and hints only — never a credential value.
//!
//! The package bundles the registry from its single committed source at
//! packaging time, so the data cannot drift from the file the dev server and
//! the other reference applications consume.

import Foundation

/// Which reference applications register a provider today.
public struct ProviderRegistrationAvailability: Decodable, Sendable, Equatable {
    /// The Android Compose reference application registers it.
    public let android: Bool
    /// The iOS SwiftUI reference application registers it.
    public let ios: Bool
    /// The React Native reference application registers it.
    public let reactNative: Bool
    /// The WebAssembly browser reference application registers it.
    public let wasm: Bool

    private enum CodingKeys: String, CodingKey {
        case android, ios, wasm
        case reactNative = "react_native"
    }
}

/// One provider descriptor of the registry's `providers` list.
public struct ProviderDescriptor: Decodable, Identifiable, Sendable, Equatable {
    /// The stable dispatch key the applications' registration arm switches on.
    public let providerKey: String
    /// The provider's display name.
    public let displayName: String
    /// The credential's generic human label.
    public let credentialLabel: String
    /// The capability names (`geocoding`, `routing`, …) this provider serves,
    /// spelled as the registry's `served_contracts` entries spell them. The
    /// settings surface offers a provider for a capability only when this
    /// list names it, so a new provider becomes selectable for a capability
    /// by editing the registry, never the view.
    public let servedContracts: [String]
    /// Which reference applications register this provider today.
    public let registrationAvailable: ProviderRegistrationAvailability
    /// The provider's attribution line (rendered verbatim by every shell).
    public let attributionText: String
    /// The provider's terms note (the attribution panel's second block).
    public let termsNote: String
    /// The public demo deployment address a keyless provider passes to the
    /// SDK as its deployment base URL, or blank when the descriptor declares
    /// none (the settings row then refuses the selection with a stated
    /// reason instead of reaching `localhost`).
    public let demoBaseURL: String
    /// The one-line note the settings row shows beside the demo address.
    public let demoBaseURLNote: String
    /// Where to create a credential, shown under the credential field.
    public let settingsHint: String
    /// The provider's raster-tile URL template (`{z}/{x}/{y}`), or blank when
    /// the provider publishes no raster source of that shape. A keyless
    /// descriptor with a template serves its own base map without any
    /// credential; the settings surface offers it in the base-map section.
    public let rasterUrlTemplate: String
    /// The provider's highest served tile zoom level (the descriptor default
    /// of zero).
    public let maximumZoomLevel: Int
    /// The provider's vector-tile URL template (`{z}/{x}/{y}`), or blank when
    /// the provider publishes no vector tileset.
    public let vectorUrlTemplate: String
    /// The tile schema the provider's vector tileset carries (`carto`,
    /// `openmaptiles`); informational, because the styles the base offers are
    /// named by the descriptor's own list.
    public let vectorStyleSchema: String
    /// The committed vector styles the registry offers for this provider, in
    /// artifact order.
    public let vectorStyles: [VectorStyleDescriptor]

    /// The descriptor's identity is its provider key (one entry per provider).
    public var id: String { providerKey }

    private enum CodingKeys: String, CodingKey {
        case providerKey = "provider_key"
        case displayName = "display_name"
        case credentialLabel = "credential_label"
        case servedContracts = "served_contracts"
        case registrationAvailable = "registration_available"
        case attributionText = "attribution_text"
        case termsNote = "terms_note"
        case demoBaseURL = "demo_base_url"
        case demoBaseURLNote = "demo_base_url_note"
        case settingsHint = "settings_hint"
        case rasterUrlTemplate = "raster_url_template"
        case maximumZoomLevel = "maximum_zoom_level"
        case vectorUrlTemplate = "url_template"
        case vectorStyleSchema = "vector_style"
        case vectorStyles = "vector_styles"
    }

    /// Decodes a descriptor tolerantly: a keyless provider declares a missing
    /// or blank `credential_label`, and the descriptor still decodes to an
    /// empty label rather than failing the whole registry.
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        providerKey = try container.decode(String.self, forKey: .providerKey)
        displayName = try container.decode(String.self, forKey: .displayName)
        credentialLabel = try container.decodeIfPresent(
            String.self,
            forKey: .credentialLabel
        ) ?? ""
        servedContracts = try container.decodeIfPresent(
            [String].self,
            forKey: .servedContracts
        ) ?? []
        registrationAvailable = try container.decode(
            ProviderRegistrationAvailability.self,
            forKey: .registrationAvailable
        )
        attributionText = try container.decode(String.self, forKey: .attributionText)
        termsNote = try container.decodeIfPresent(String.self, forKey: .termsNote) ?? ""
        demoBaseURL = try container.decodeIfPresent(
            String.self,
            forKey: .demoBaseURL
        ) ?? ""
        demoBaseURLNote = try container.decodeIfPresent(
            String.self,
            forKey: .demoBaseURLNote
        ) ?? ""
        settingsHint = try container.decode(String.self, forKey: .settingsHint)
        rasterUrlTemplate = try container.decodeIfPresent(
            String.self,
            forKey: .rasterUrlTemplate
        ) ?? ""
        maximumZoomLevel = try container.decodeIfPresent(
            Int.self,
            forKey: .maximumZoomLevel
        ) ?? 0
        vectorUrlTemplate = try container.decodeIfPresent(
            String.self,
            forKey: .vectorUrlTemplate
        ) ?? ""
        vectorStyleSchema = try container.decodeIfPresent(
            String.self,
            forKey: .vectorStyleSchema
        ) ?? ""
        vectorStyles = try container.decodeIfPresent(
            [VectorStyleDescriptor].self,
            forKey: .vectorStyles
        ) ?? []
    }

    /// Whether the descriptor declares a public demo deployment address a
    /// keyless selection can hand the SDK.
    public var hasDemoBaseURL: Bool {
        !demoBaseURL.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    /// Whether this platform can register the provider as a base map without
    /// a credential: the provider-neutral URL-template raster arm exists on
    /// iOS and the descriptor publishes a template.
    public var isCredentiallessBaseMapAvailableOnIOS: Bool {        !rasterUrlTemplate.trimmingCharacters(
            in: .whitespacesAndNewlines
        ).isEmpty && registrationAvailable.ios
    }

    /// Whether the provider offers a styled vector base: a vector tileset URL
    /// template plus at least one committed style. The vector base is served
    /// by the shipped `MapView` on every platform through the additive
    /// contract member, so no registration arm gates it.
    public var offersVectorBase: Bool {
        !vectorUrlTemplate.trimmingCharacters(
            in: .whitespacesAndNewlines
        ).isEmpty && !vectorStyles.isEmpty
    }

    /// Whether the provider offers a styled vector base from style documents
    /// it publishes: it declares the vector-tile contract, this platform
    /// registers it, and it names at least one style. The rows are listed
    /// whether or not the credential is applied; without it they state
    /// `Needs <credential label>` and cannot be selected.
    public var offersHostedVectorBase: Bool {
        servedContracts.contains("vector_tiles")
            && !vectorStyles.isEmpty
            && registrationAvailable.ios
    }
}

/// One vector style a registry descriptor offers: the committed style id the
/// SDK paints the base with, the consumer-facing style name the base-map menu
/// shows, the theme the style belongs to (`light`/`dark`), and the attribution
/// line the style's design requires.
public struct VectorStyleDescriptor: Decodable, Sendable, Equatable, Identifiable {
    /// The committed style id the SDK paints with (`bright-light`, …).
    public let styleId: String
    /// Consumer-facing style name (`Bright`, `Fiord`, `Positron`).
    public let displayName: String
    /// The theme the style was designed on.
    public let theme: String
    /// The attribution line the style's design requires.
    public let attributionText: String
    /// The provider-published style document this style paints with, or blank
    /// when the SDK carries the style. A hosted document is fetched with the
    /// registered credential and kept for the session, so the application
    /// ships no vendor style file.
    public let documentUrl: String

    /// One entry per style id within a descriptor.
    public var id: String { styleId }

    /// Builds one style record; the document address defaults to none for a
    /// committed style.
    public init(
        styleId: String,
        displayName: String,
        theme: String,
        attributionText: String,
        documentUrl: String = ""
    ) {
        self.styleId = styleId
        self.displayName = displayName
        self.theme = theme
        self.attributionText = attributionText
        self.documentUrl = documentUrl
    }

    private enum CodingKeys: String, CodingKey {
        case styleId = "style_id"
        case displayName = "display_name"
        case theme
        case attributionText = "attribution_text"
        case documentUrl = "document_url"
    }

    /// Decodes a style tolerantly: a committed style declares no document.
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        styleId = try container.decode(String.self, forKey: .styleId)
        displayName = try container.decode(String.self, forKey: .displayName)
        theme = try container.decode(String.self, forKey: .theme)
        attributionText = try container.decode(String.self, forKey: .attributionText)
        documentUrl = try container.decodeIfPresent(
            String.self,
            forKey: .documentUrl
        ) ?? ""
    }
}

/// The styled vector base a keyless start opens on: the provider key and
/// committed style id the registry names at its top level. The reference
/// application resolves it against the descriptors, so an incomplete
/// declaration falls back to the credential-less raster source rather than
/// installing nothing.
public struct KeylessBaseMapDescriptor: Decodable, Sendable, Equatable {
    /// The provider whose vector tileset the base uses.
    public let providerKey: String
    /// The committed style id the base paints with.
    public let styleId: String

    private enum CodingKeys: String, CodingKey {
        case providerKey = "provider_key"
        case styleId = "style_id"
    }

    /// Whether both halves name something (a blank declaration is treated as
    /// absent, so the keyless start keeps the credential-less raster source).
    var isComplete: Bool {
        !providerKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !styleId.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }
}

/// The decoded shared provider registry.
public struct ProviderRegistryCatalog: Decodable, Sendable {
    /// The registry schema version the artifact declares.
    public let schemaVersion: Int
    /// The `User-Agent` template a credential-less base map identifies the
    /// reference app with (the `%s` carries the compiled SDK version), or
    /// blank when the artifact declares none (the SDK then sends no header).
    public let referenceAppUserAgentTemplate: String
    /// The provider descriptors in artifact order.
    public let providers: [ProviderDescriptor]
    /// The styled vector base a keyless start opens on, or nil when the
    /// artifact declares none (the keyless start then keeps the
    /// credential-less raster source).
    public let keylessBaseMap: KeylessBaseMapDescriptor?

    private enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case referenceAppUserAgentTemplate = "reference_app_user_agent_template"
        case keylessBaseMap = "keyless_base_map"
        case providers
    }

    /// Decodes the registry tolerantly: an artifact without the User-Agent
    /// template still decodes, with a blank template (no header sent).
    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        schemaVersion = try container.decode(Int.self, forKey: .schemaVersion)
        referenceAppUserAgentTemplate = try container.decodeIfPresent(
            String.self,
            forKey: .referenceAppUserAgentTemplate
        ) ?? ""
        providers = try container.decode([ProviderDescriptor].self, forKey: .providers)
        let declaredKeylessBaseMap = try container.decodeIfPresent(
            KeylessBaseMapDescriptor.self,
            forKey: .keylessBaseMap
        )
        keylessBaseMap = declaredKeylessBaseMap?.isComplete == true
            ? declaredKeylessBaseMap
            : nil
    }

    /// Decoding/loading failures of the registry, carrying the artifact's own
    /// naming so an application presents the failure honestly.
    public enum ProviderRegistryCatalogError: Swift.Error, LocalizedError {
        /// The registry resource was not found in the loaded bundle.
        case registryResourceMissing(bundleDescription: String)
        /// The registry JSON did not decode (the decoder's failure detail is
        /// echoed verbatim).
        case registryDecodeFailed(underlying: String)

        public var errorDescription: String? {
            switch self {
            case let .registryResourceMissing(bundleDescription):
                return "the providers.json registry was not found in \(bundleDescription)"
            case let .registryDecodeFailed(underlying):
                return "the providers.json registry failed to decode: \(underlying)"
            }
        }
    }

    /// The name the registry ships under in the processed package resource.
    static let registryResourceName = "providers"

    /// The name the registry file ships under in the processed package resource.
    static let registryResourceFileExtension = "json"

    /// Loads the registry from the package's bundled resource, refreshed from
    /// the shared registry at packaging time.
    public static func loadedFromPackageResource() throws -> ProviderRegistryCatalog {
        let packageResourceBundle = Bundle.module
        guard let registryURL = packageResourceBundle.url(
            forResource: registryResourceName,
            withExtension: registryResourceFileExtension
        ) else {
            throw ProviderRegistryCatalogError.registryResourceMissing(
                bundleDescription:
                    packageResourceBundle.bundleIdentifier ?? "an unnamed bundle"
            )
        }
        do {
            let registryData = try Data(contentsOf: registryURL)
            return try decoded(from: registryData)
        } catch {
            throw ProviderRegistryCatalogError.registryDecodeFailed(
                underlying: String(describing: error)
            )
        }
    }

    /// Decodes the registry's JSON (the structs carry the snake_case keys
    /// explicitly in their CodingKeys).
    public static func decoded(from data: Data) throws -> ProviderRegistryCatalog {
        try JSONDecoder().decode(ProviderRegistryCatalog.self, from: data)
    }
}
