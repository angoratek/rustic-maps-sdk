//! The contract-owned registry-name mapping's tests: every capability name the
//! shared provider registry declares resolves to a contract, and the name the
//! contract reports for that contract is the registry's own spelling. A
//! capability added to the registry alone is therefore covered without a
//! per-surface table.

import MapNativeCoreUI
import RusticMapsSDK
import XCTest

final class ProviderContractRegistryNamesTests: XCTestCase {
    /// Every name the bundled shared registry declares across its provider
    /// descriptors.
    private func registryDeclaredCapabilityNames() throws -> Set<String> {
        let catalog = try ProviderRegistryCatalog.loadedFromPackageResource()
        return Set(catalog.providers.flatMap(\.servedContracts))
    }

    func testEveryRegistryDeclaredCapabilityMapsToAContract() throws {
        let registryNames = try registryDeclaredCapabilityNames()

        XCTAssertFalse(
            registryNames.isEmpty,
            "the shared registry must declare at least one capability"
        )
        for registryName in registryNames {
            let contract = try XCTUnwrap(
                providerContractFromRegistryName(registryName: registryName),
                "the registry declares '\(registryName)' but no contract maps it"
            )
            XCTAssertEqual(
                registryNameForProviderContract(contract: contract),
                registryName,
                "the contract must report the registry's own spelling"
            )
        }
    }

    func testAnUnknownRegistryNameHasNoContract() {
        XCTAssertNil(
            providerContractFromRegistryName(registryName: "a-future-capability")
        )
        XCTAssertNil(providerContractFromRegistryName(registryName: ""))
    }
}
