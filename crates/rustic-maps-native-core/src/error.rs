//! The error model of the native binding surface: a fielded mirror of the
//! `geospatial-types` `SDKError` variants the contract's calls can actually
//! produce, carrying their verbatim detail messages across the FFI.

use geospatial_types::SDKError;

/// Failures thrown across the native binding surface of rustic-maps-native-core.
///
/// The camera and projection calls produce
/// [`SdkError::InvalidCoordinateInput`] only; the async provider surface
/// additionally produces the transport, parsing, application-level,
/// rate-limit and not-configured variants below; the navigation surface
/// produces [`SdkError::InvalidNavigationInput`] for route records and
/// navigation policies failing native validation. The variants mirror the
/// `SDKError` subset the contract's calls return (see
/// `src/rustic_maps.udl`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdkError {
    /// A coordinate, viewport extent or box corner failed validation; the
    /// detail carries the verbatim message of the failing core validator.
    InvalidCoordinateInput {
        /// Verbatim message of the failing validator.
        detail: String,
    },
    /// The network transport failed (connection, timeout, or DNS); the
    /// detail carries the provider's verbatim message.
    NetworkRequestFailed {
        /// Verbatim transport-failure message.
        detail: String,
    },
    /// The provider responded with a payload the parser could not use; the
    /// detail carries the verbatim parsing failure.
    ResponseParsingFailure {
        /// Verbatim parsing-failure message.
        detail: String,
    },
    /// The provider returned a documented application-level failure status;
    /// the detail carries its verbatim message.
    ProviderRequestRejected {
        /// Verbatim application-level rejection message.
        detail: String,
    },
    /// The token-bucket rate limiter is exhausted; the caller retries after
    /// the next refill.
    RateLimitExhausted,
    /// No provider implementation is registered for the requested contract.
    ProviderNotConfigured {
        /// Name of the unregistered contract, e.g. `GeocodingProvider`.
        contract_name: String,
    },
    /// A navigation input failed native-boundary validation; either a policy
    /// carried an unusable threshold (non-finite or negative) or a route
    /// record carried a step maneuver label the converter cannot map onto the
    /// native maneuver classification. Unlike the camera surface this variant
    /// is not a mirror of a core `SDKError` variant: navigation policies and
    /// route records enter the SDK at this boundary and have no core-side
    /// validator of their own yet.
    InvalidNavigationInput {
        /// Verbatim description of the refused navigation input.
        detail: String,
    },
    /// An overlay descriptor did not describe a usable overlay (wrong kind
    /// payload, an out-of-range color channel, or a non-positive or
    /// non-finite width or radius) or a surface input was structurally
    /// unusable (a null or ambiguous platform surface pointer, or a
    /// non-positive viewport extent). Unlike the camera surface this variant
    /// is not a mirror of a core `SDKError` variant: overlay descriptors and
    /// surface handles enter the SDK at the surface-view boundary and have
    /// no core-side validator of their own.
    InvalidRenderInput {
        /// Verbatim description of the refused surface input.
        detail: String,
    },
    /// The live rendering could not proceed on the platform surface: the
    /// platform handle or surface was unusable, the graphics context could
    /// not be acquired, the surface format could not be negotiated or
    /// configured, the frame texture could not be acquired even after the
    /// stale-surface retry, the draw list would overflow, or the frame's
    /// resources were busy in another draw. Unlike the camera surface this
    /// variant is not a mirror of a core `SDKError` variant: it carries the
    /// surface renderer's own failures across the binding surface.
    RenderingSurfaceUnavailable {
        /// Verbatim description of the unavailable rendering surface.
        detail: String,
    },
    /// A cache read, write or eviction failed: an unavailable store, a
    /// corrupt entry or a byte-budget refusal. Mirrors the core's
    /// `CacheOperationFailure`; the detail names the reason verbatim.
    CacheOperationFailure {
        /// Verbatim description of the failing cache operation.
        detail: String,
    },
    /// An offline routing graph failed validation or could not be read.
    /// Mirrors the core's `OfflineRoutingFailure`; the detail names the
    /// reason verbatim.
    OfflineRoutingFailure {
        /// Verbatim description of the refused offline graph.
        detail: String,
    },
}

impl std::fmt::Display for SdkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCoordinateInput { detail } => {
                write!(formatter, "invalid coordinate input: {detail}")
            }
            Self::NetworkRequestFailed { detail } => {
                write!(formatter, "network request failed: {detail}")
            }
            Self::ResponseParsingFailure { detail } => {
                write!(formatter, "response parsing failure: {detail}")
            }
            Self::ProviderRequestRejected { detail } => {
                write!(formatter, "provider rejected request: {detail}")
            }
            Self::RateLimitExhausted => write!(formatter, "rate limit exhausted"),
            Self::ProviderNotConfigured { contract_name } => {
                write!(formatter, "no provider configured for {contract_name}")
            }
            Self::InvalidNavigationInput { detail } => {
                write!(formatter, "invalid navigation input: {detail}")
            }
            Self::InvalidRenderInput { detail } => {
                write!(formatter, "invalid render input: {detail}")
            }
            Self::RenderingSurfaceUnavailable { detail } => {
                write!(formatter, "rendering surface unavailable: {detail}")
            }
            Self::CacheOperationFailure { detail } => {
                write!(formatter, "cache operation failure: {detail}")
            }
            Self::OfflineRoutingFailure { detail } => {
                write!(formatter, "offline routing failure: {detail}")
            }
        }
    }
}

impl std::error::Error for SdkError {}

/// The verbatim detail prefix the unclassified-foreign-callback conversion
/// writes (see the `From<uniffi::UnexpectedUniFFICallbackError>` impl below).
/// Single source of truth for both that conversion and the provider-failure
/// diagnostic seam's adapter-side classifier, which recognizes the
/// conversion's subject by this exact prefix — the one piece of Rust-side
/// coordination between the two sites.
pub(crate) const UNMAPPED_FOREIGN_CALLBACK_DETAIL_PREFIX: &str =
    "the provider implementation failed without a mapped error: ";

impl SdkError {
    /// Classifies this failure for the provider-failure diagnostic seam and
    /// is called from Rust only; the public error contract is unchanged.
    /// When this failure is the converted form of an unclassified foreign
    /// callback failure, the callback machinery's verbatim reason embedded
    /// under the conversion's detail prefix; `None` for every failure that
    /// did not come from that conversion, in particular a foreign
    /// implementation's own typed `SdkError` throw.
    pub(crate) fn unclassified_foreign_callback_reason(&self) -> Option<&str> {
        match self {
            Self::ProviderRequestRejected { detail } => {
                detail.strip_prefix(UNMAPPED_FOREIGN_CALLBACK_DETAIL_PREFIX)
            }
            _ => Option::None,
        }
    }
}

impl From<uniffi::UnexpectedUniFFICallbackError> for SdkError {
    /// Maps a foreign provider implementation's *unclassified* failure onto
    /// the binding error model. A foreign implementation of a provider
    /// callback contract that throws its contract's `SdkError` takes the
    /// typed error round trip exactly (`lift_error`); a foreign impl that
    /// fails some other way (any exception the contract does not spell) is
    /// reported by the foreign vtable as an unexpected callback failure.
    /// Without this conversion uniffi would turn that report into a Rust
    /// panic — correctly contained by the FFI scaffolding's panic boundary
    /// (see [`crate::panic_boundary`]) but only as the untyped
    /// "unexpected error" status. With it, the provider contracts' async
    /// exports resolve with a typed failure: this is the closest
    /// existing variant — the failing party is the provider implementation —
    /// carrying the foreign failure's verbatim reason verbatim.
    ///
    /// The conversion is Rust-only: the UDL surface and the enum's contract
    /// variants are unchanged.
    fn from(unexpected_callback_error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::ProviderRequestRejected {
            detail: format!(
                "{UNMAPPED_FOREIGN_CALLBACK_DETAIL_PREFIX}{}",
                unexpected_callback_error.reason
            ),
        }
    }
}

impl From<SDKError> for SdkError {
    /// Maps a core `SDKError` onto the binding error model, preserving every
    /// variant one-to-one with its verbatim detail. Each core failure has a
    /// matching binding variant, so no failure is folded into an unrelated
    /// one and the conversion never panics.
    fn from(sdk_error: SDKError) -> Self {
        match sdk_error {
            SDKError::InvalidCoordinateInput(detail) => Self::InvalidCoordinateInput { detail },
            SDKError::NetworkRequestFailed(detail) => Self::NetworkRequestFailed { detail },
            SDKError::ResponseParsingFailure(detail) => Self::ResponseParsingFailure { detail },
            SDKError::ProviderRequestRejected(detail) => Self::ProviderRequestRejected { detail },
            SDKError::RateLimitExhausted => Self::RateLimitExhausted,
            SDKError::ProviderNotConfigured(contract_name) => {
                Self::ProviderNotConfigured { contract_name }
            }
            SDKError::CacheOperationFailure(reason) => Self::CacheOperationFailure {
                detail: format!("{reason:?}"),
            },
            SDKError::OfflineRoutingFailure(reason) => Self::OfflineRoutingFailure {
                detail: format!("{reason:?}"),
            },
            // Every core variant now has its own binding variant, so there is
            // no catch-all: a new core failure must choose its surface
            // deliberately rather than be folded into a coordinate error.
        }
    }
}

impl From<SdkError> for SDKError {
    /// Maps the binding error back onto the core error model, exact for the
    /// six `SDKError`-mirroring variants: the forward
    /// `From<SDKError> for SdkError` conversion and this one round-trip them
    /// without information loss. This is the direction the foreign-callback
    /// provider adapters need — a foreign implementation throws `SdkError`
    /// across the FFI and the adapter re-raises it as the core error of the
    /// implemented provider contract. The navigation-only
    /// [`SdkError::InvalidNavigationInput`] has no core equivalent (nothing
    /// re-raises a navigation-input failure into a provider contract), so it
    /// carries its detail under `InvalidCoordinateInput` to keep the
    /// conversion total and the failure diagnosable.
    fn from(sdk_error: SdkError) -> Self {
        match sdk_error {
            SdkError::InvalidCoordinateInput { detail } => Self::InvalidCoordinateInput(detail),
            SdkError::NetworkRequestFailed { detail } => Self::NetworkRequestFailed(detail),
            SdkError::ResponseParsingFailure { detail } => Self::ResponseParsingFailure(detail),
            SdkError::ProviderRequestRejected { detail } => Self::ProviderRequestRejected(detail),
            SdkError::RateLimitExhausted => Self::RateLimitExhausted,
            SdkError::ProviderNotConfigured { contract_name } => {
                Self::ProviderNotConfigured(contract_name)
            }
            // Navigation-only variant with no core equivalent (see the impl
            // documentation); the detail keeps the failure diagnosable.
            SdkError::InvalidNavigationInput { detail } => Self::InvalidCoordinateInput(detail),
            // Render-only variants with no core equivalent (see the impl
            // documentation); the details keep the failures diagnosable, the
            // same folding the navigation-only variant uses.
            SdkError::InvalidRenderInput { detail } => Self::InvalidCoordinateInput(detail),
            SdkError::RenderingSurfaceUnavailable { detail } => {
                Self::InvalidCoordinateInput(detail)
            }
            // The cache and offline reasons are richer than their core
            // enums' round-trip through a string, so the returning direction
            // keeps the detail diagnosable under the input variant, like the
            // other FFI-only variants above.
            SdkError::CacheOperationFailure { detail } => Self::InvalidCoordinateInput(detail),
            SdkError::OfflineRoutingFailure { detail } => Self::InvalidCoordinateInput(detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SDKError, SdkError, UNMAPPED_FOREIGN_CALLBACK_DETAIL_PREFIX};

    /// Display text `SDKError` uses for the rate-limit variant.
    const RATE_LIMIT_DISPLAY_TEXT: &str = "rate limit exhausted";
    /// The callback machinery's reason the classification tests embed.
    const UNCLASSIFIED_REASON: &str = "kotlin.KotlinNullPointerException: provider dead";
    /// A typed rejection detail a foreign implementation itself throws.
    const MAPPED_REJECTION_DETAIL: &str = "quota exceeded";

    #[test]
    fn a_cache_failure_is_not_reported_as_bad_coordinates() {
        let cache_failure: SdkError = geospatial_types::SDKError::CacheOperationFailure(
            geospatial_types::CacheOperationFailureReason::StoreUnavailable,
        )
        .into();
        match cache_failure {
            SdkError::CacheOperationFailure { detail } => {
                assert!(
                    detail.contains("StoreUnavailable"),
                    "the reason survives into the detail, got {detail:?}"
                );
            }
            other => panic!(
                "a cache failure must keep its own variant, not fold into an input error: {other:?}"
            ),
        }
    }

    #[test]
    fn an_offline_graph_failure_keeps_its_own_variant() {
        let offline_failure: SdkError = geospatial_types::SDKError::OfflineRoutingFailure(
            geospatial_types::OfflineGraphInvalidReason::VersionUnsupported,
        )
        .into();
        assert!(
            matches!(offline_failure, SdkError::OfflineRoutingFailure { .. }),
            "an offline-graph failure must not be reported as bad coordinates: {offline_failure:?}"
        );
    }

    #[test]
    fn unclassified_callback_conversion_carries_the_reason_verbatim() {
        let converted: SdkError =
            uniffi::UnexpectedUniFFICallbackError::new(UNCLASSIFIED_REASON).into();
        match converted {
            SdkError::ProviderRequestRejected { detail } => {
                assert_eq!(
                    detail,
                    format!("{UNMAPPED_FOREIGN_CALLBACK_DETAIL_PREFIX}{UNCLASSIFIED_REASON}"),
                    "the conversion's detail keeps its frozen phrasing verbatim"
                );
            }
            other => panic!("the conversion must produce the rejection variant, got {other:?}"),
        }
    }

    #[test]
    fn unclassified_callback_conversion_classifies_with_the_embedded_reason() {
        let converted: SdkError =
            uniffi::UnexpectedUniFFICallbackError::new(UNCLASSIFIED_REASON).into();
        assert_eq!(
            converted.unclassified_foreign_callback_reason(),
            Some(UNCLASSIFIED_REASON),
            "the classifier extracts the verbatim callback reason"
        );
    }

    #[test]
    fn typed_failures_never_classify_as_unclassified() {
        for sdk_error in [
            SdkError::ProviderRequestRejected {
                detail: MAPPED_REJECTION_DETAIL.to_string(),
            },
            SdkError::NetworkRequestFailed {
                detail: "offline".to_string(),
            },
            SdkError::RateLimitExhausted,
        ] {
            assert!(
                sdk_error.unclassified_foreign_callback_reason().is_none(),
                "a mapped failure must not classify as unclassified: {sdk_error:?}"
            );
        }
    }

    #[test]
    fn invalid_coordinate_input_maps_onto_the_binding_variant_with_verbatim_detail() {
        let sdk_error =
            SDKError::InvalidCoordinateInput("latitude 91 outside [-90, 90]".to_string());
        let mapped: SdkError = sdk_error.into();
        assert_eq!(
            mapped,
            SdkError::InvalidCoordinateInput {
                detail: "latitude 91 outside [-90, 90]".to_string(),
            }
        );
    }

    #[test]
    fn provider_failures_map_onto_their_binding_variants_one_to_one() {
        let network_mapped: SdkError = SDKError::NetworkRequestFailed("offline".to_string()).into();
        match network_mapped {
            SdkError::NetworkRequestFailed { detail } => {
                assert_eq!(detail, "offline");
            }
            other => panic!("network failure maps to the wrong variant: {other:?}"),
        }

        let parsing_mapped: SdkError =
            SDKError::ResponseParsingFailure("bad json".to_string()).into();
        assert!(matches!(
            parsing_mapped,
            SdkError::ResponseParsingFailure { .. }
        ));

        let rejection_mapped: SdkError =
            SDKError::ProviderRequestRejected("quota exceeded".to_string()).into();
        assert!(matches!(
            rejection_mapped,
            SdkError::ProviderRequestRejected { .. }
        ));

        let rate_limited_mapped: SdkError = SDKError::RateLimitExhausted.into();
        assert_eq!(rate_limited_mapped, SdkError::RateLimitExhausted);
        assert_eq!(rate_limited_mapped.to_string(), RATE_LIMIT_DISPLAY_TEXT);

        let not_configured_mapped: SdkError =
            SDKError::ProviderNotConfigured("GeocodingProvider".to_string()).into();
        assert_eq!(
            not_configured_mapped,
            SdkError::ProviderNotConfigured {
                contract_name: "GeocodingProvider".to_string()
            }
        );
        assert_eq!(
            not_configured_mapped.to_string(),
            "no provider configured for GeocodingProvider"
        );
    }

    #[test]
    fn cache_and_offline_failures_keep_their_own_variants() {
        // Round 42 stopped folding these into `InvalidCoordinateInput`: a
        // cache store that cannot be opened is not bad coordinates, and the
        // consumer needs to tell them apart to react.
        let offline: SdkError = SDKError::OfflineRoutingFailure(
            geospatial_types::OfflineGraphInvalidReason::ArchiveMalformed,
        )
        .into();
        assert_eq!(
            offline.to_string(),
            "offline routing failure: ArchiveMalformed"
        );
        let cache: SdkError = SDKError::CacheOperationFailure(
            geospatial_types::CacheOperationFailureReason::WriteRejected,
        )
        .into();
        assert_eq!(cache.to_string(), "cache operation failure: WriteRejected");
    }

    #[test]
    fn navigation_only_input_variant_keeps_its_detail_under_the_core_coordinate_variant() {
        let navigation_detail = "step-advance threshold must be finite and non-negative";
        let mapped: SDKError = SdkError::InvalidNavigationInput {
            detail: navigation_detail.to_string(),
        }
        .into();
        assert_eq!(
            mapped,
            SDKError::InvalidCoordinateInput(navigation_detail.to_string()),
            "the navigation-only variant has no core equivalent and carries its detail"
        );
    }

    #[test]
    fn render_only_variants_keep_their_details_under_the_core_coordinate_variant() {
        let render_detail = "overlay 0 exceeds the color channel range";
        let mapped: SDKError = SdkError::InvalidRenderInput {
            detail: render_detail.to_string(),
        }
        .into();
        assert_eq!(
            mapped,
            SDKError::InvalidCoordinateInput(render_detail.to_string()),
            "the render-only variant has no core equivalent and carries its detail"
        );

        let surface_detail = "the graphics adapter unavailable: no device";
        let mapped: SDKError = SdkError::RenderingSurfaceUnavailable {
            detail: surface_detail.to_string(),
        }
        .into();
        assert_eq!(
            mapped,
            SDKError::InvalidCoordinateInput(surface_detail.to_string()),
            "the surface-only variant has no core equivalent and carries its detail"
        );
    }

    #[test]
    fn binding_variants_round_trip_through_the_core_error_model() {
        for sdk_error in [
            SdkError::InvalidCoordinateInput {
                detail: "latitude 91 outside [-90, 90]".to_string(),
            },
            SdkError::NetworkRequestFailed {
                detail: "offline".to_string(),
            },
            SdkError::ResponseParsingFailure {
                detail: "bad json".to_string(),
            },
            SdkError::ProviderRequestRejected {
                detail: "quota exceeded".to_string(),
            },
            SdkError::RateLimitExhausted,
            SdkError::ProviderNotConfigured {
                contract_name: "GeocodingProvider".to_string(),
            },
        ] {
            let native_sdk_error: SDKError = sdk_error.clone().into();
            let round_tripped: SdkError = native_sdk_error.into();
            assert_eq!(
                round_tripped, sdk_error,
                "variant {sdk_error:?} must round-trip"
            );
        }
    }
}
