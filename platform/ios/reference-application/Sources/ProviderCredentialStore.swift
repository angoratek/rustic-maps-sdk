//! The opt-in secure storage of provider credentials
//! (docs/ReferenceAppUX.md §6.5): one Keychain generic-password item per
//! provider key, written only while the settings view's "Remember on this
//! device" toggle is on. There is no plaintext fallback — when the Keychain
//! refuses a request the failure is reported to the settings surface
//! verbatim and the credential stays session-only. The credential value is
//! never logged, echoed in an error, or carried in a thrown error's text.

import Foundation
import Security

/// The Keychain-backed store of provider credentials (§6.5).
struct ProviderCredentialStore {
    // MARK: Named constants

    /// The Keychain service all provider-credential items live under (the
    /// reference application's documented service identifier).
    static let serviceName = "com.angoratek.rustic-maps.refapp.provider-credentials"

    /// The Keychain service this store instance reads and writes.
    private let serviceName: String

    /// The Keychain item class: one generic password per provider key.
    private static let itemClass: CFString = kSecClassGenericPassword

    /// The item's protection class: readable only while the device is
    /// unlocked, and never migrated to another device (the credential is
    /// this device's opt-in, not a synchronizable secret).
    private static let accessibleAttribute: CFString =
        kSecAttrAccessibleWhenUnlockedThisDeviceOnly

    /// Builds the store over the reference application's Keychain service.
    init(serviceName: String = ProviderCredentialStore.serviceName) {
        self.serviceName = serviceName
    }

    // MARK: Reads and writes

    /// Stores (or replaces) the provider's credential in the Keychain.
    /// Throws the Keychain's honest refusal; the credential never appears in
    /// the error.
    func save(credential: String, forProviderKey providerKey: String) throws {
        let credentialData = Data(credential.utf8)
        let lookupAttributes = Self.lookupAttributes(
            serviceName: serviceName, providerKey: providerKey
        )
        let updateAttributes: [CFString: Any] = [
            kSecValueData: credentialData,
            kSecAttrAccessible: Self.accessibleAttribute,
        ]
        let updateStatus = SecItemUpdate(
            lookupAttributes as CFDictionary, updateAttributes as CFDictionary
        )
        switch updateStatus {
        case errSecSuccess:
            return
        case errSecItemNotFound:
            var addAttributes = lookupAttributes
            addAttributes[kSecValueData] = credentialData
            addAttributes[kSecAttrAccessible] = Self.accessibleAttribute
            let addStatus = SecItemAdd(addAttributes as CFDictionary, nil)
            guard addStatus == errSecSuccess else {
                throw ProviderCredentialStoreError.keychainRejected(
                    operation: "store", status: addStatus
                )
            }
        default:
            throw ProviderCredentialStoreError.keychainRejected(
                operation: "store", status: updateStatus
            )
        }
    }

    /// Reads the provider's stored credential, or nil when the user never
    /// opted in (or cleared it). Throws the Keychain's honest refusal.
    func credential(forProviderKey providerKey: String) throws -> String? {
        var query = Self.lookupAttributes(
            serviceName: serviceName, providerKey: providerKey
        )
        query[kSecReturnData] = true
        query[kSecMatchLimit] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        switch status {
        case errSecSuccess:
            guard
                let credentialData = result as? Data,
                let credential = String(data: credentialData, encoding: .utf8)
            else {
                throw ProviderCredentialStoreError.storedValueIsNotText
            }
            return credential.isEmpty ? nil : credential
        case errSecItemNotFound:
            return nil
        default:
            throw ProviderCredentialStoreError.keychainRejected(
                operation: "read", status: status
            )
        }
    }

    /// Deletes the provider's stored credential. A missing item succeeds
    /// (the clear's intent is already met).
    func deleteCredential(forProviderKey providerKey: String) throws {
        let status = SecItemDelete(
            Self.lookupAttributes(
                serviceName: serviceName, providerKey: providerKey
            ) as CFDictionary
        )
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw ProviderCredentialStoreError.keychainRejected(
                operation: "delete", status: status
            )
        }
    }

    // MARK: Query construction

    /// The attributes that identify one provider's item (the service and
    /// the provider key as its account).
    private static func lookupAttributes(
        serviceName: String, providerKey: String
    ) -> [CFString: Any] {
        [
            kSecClass: itemClass,
            kSecAttrService: serviceName,
            kSecAttrAccount: providerKey,
        ]
    }
}

/// The store's honest failures: the operating system's status and message,
/// never the credential value.
enum ProviderCredentialStoreError: Swift.Error, LocalizedError {
    /// The Keychain refused a request with the given operating-system
    /// status.
    case keychainRejected(operation: String, status: OSStatus)
    /// The stored item is not a UTF-8 text credential.
    case storedValueIsNotText

    var errorDescription: String? {
        switch self {
        case let .keychainRejected(operation, status):
            let statusMessage = SecCopyErrorMessageString(status, nil)
                as String? ?? "unknown Keychain error"
            return "the Keychain \(operation) request failed (status \(status)): \(statusMessage)"
        case .storedValueIsNotText:
            return "the Keychain item does not hold a text credential"
        }
    }
}
