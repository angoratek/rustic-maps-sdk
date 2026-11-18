package com.rusticmaps.sdk.referenceapplication

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Outcome of one secure-store save. */
sealed interface ProviderCredentialSaveOutcome {
    /** The credential is encrypted and persisted. */
    data object Stored : ProviderCredentialSaveOutcome

    /** Nothing was written; [reason] is shown honestly to the user. */
    data class Unavailable(val reason: String) : ProviderCredentialSaveOutcome
}

/** Outcome of one secure-store read. */
sealed interface ProviderCredentialLoadOutcome {
    /** A previously opted-in credential was decrypted. */
    data class Loaded(val credential: String) : ProviderCredentialLoadOutcome

    /** The user never opted in (or already cleared it): session-only. */
    data object Absent : ProviderCredentialLoadOutcome

    /** A stored entry exists but cannot be read; [reason] is shown honestly. */
    data class Unavailable(val reason: String) : ProviderCredentialLoadOutcome
}

/**
 * Opt-in persistent storage of provider credentials
 * (docs/ReferenceAppUX.md §6.5; docs/Providers.md
 * §6): an AES/GCM key generated inside the Android Keystore encrypts each
 * credential, and the ciphertext plus its initialization vector live in this
 * application's private `SharedPreferences`.
 *
 * Security rules enforced here:
 * - there is no plaintext fallback — any keystore or cipher failure returns
 *   [ProviderCredentialSaveOutcome.Unavailable] / [ProviderCredentialLoadOutcome.Unavailable]
 *   so the caller can say honestly that the value was not stored;
 * - the credential value is never logged, echoed or included in a reason
 *   string (an exception's own message is used verbatim and carries cipher
 *   diagnostics only);
 * - writes use `commit()` so the caller learns whether persistence actually
 *   happened before the UI claims it.
 *
 * The store is only consulted behind the settings toggle; a keyless start
 * reads nothing.
 */
class ProviderCredentialStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(
        PREFERENCES_NAME,
        Context.MODE_PRIVATE,
    )

    /**
     * Encrypts and persists one provider credential. The initialization vector
     * is stored beside the ciphertext (GCM requires a fresh IV per
     * encryption; the keystore generates it).
     */
    fun save(providerKey: String, credential: String): ProviderCredentialSaveOutcome = try {
        val cipher = Cipher.getInstance(CIPHER_TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, secretKeyOrCreate())
        val ciphertext = cipher.doFinal(credential.toByteArray(Charsets.UTF_8))
        val wasPersisted = preferences.edit()
            .putString(ciphertextPreferenceKey(providerKey), encodeBase64(ciphertext))
            .putString(initializationVectorPreferenceKey(providerKey), encodeBase64(cipher.iv))
            .commit()
        if (wasPersisted) {
            ProviderCredentialSaveOutcome.Stored
        } else {
            ProviderCredentialSaveOutcome.Unavailable(
                "the device storage refused the write",
            )
        }
    } catch (failure: Exception) {
        ProviderCredentialSaveOutcome.Unavailable(describeFailure(failure))
    }

    /**
     * Decrypts the stored credential for one provider, when the user opted in
     * before.
     *
     * Every failure — a mis-typed preference entry, a missing keystore key, a
     * keystore provider that cannot load, a bad ciphertext — is reported as
     * [ProviderCredentialLoadOutcome.Unavailable] with an honest reason. The
     * caller's startup must never crash because the secure store is unusable.
     */
    fun load(providerKey: String): ProviderCredentialLoadOutcome = try {
        val ciphertextText = preferences.getString(ciphertextPreferenceKey(providerKey), null)
        if (ciphertextText == null) {
            ProviderCredentialLoadOutcome.Absent
        } else {
            val initializationVectorText = preferences.getString(
                initializationVectorPreferenceKey(providerKey),
                null,
            )
            if (initializationVectorText == null) {
                ProviderCredentialLoadOutcome.Unavailable("the stored credential is incomplete")
            } else {
                val secretKey = existingSecretKey()
                if (secretKey == null) {
                    ProviderCredentialLoadOutcome.Unavailable("the device keystore key is missing")
                } else {
                    val cipher = Cipher.getInstance(CIPHER_TRANSFORMATION)
                    cipher.init(
                        Cipher.DECRYPT_MODE,
                        secretKey,
                        GCMParameterSpec(AUTHENTICATION_TAG_BITS, decodeBase64(initializationVectorText)),
                    )
                    val credential = cipher.doFinal(decodeBase64(ciphertextText)).toString(Charsets.UTF_8)
                    ProviderCredentialLoadOutcome.Loaded(credential)
                }
            }
        }
    } catch (failure: Exception) {
        ProviderCredentialLoadOutcome.Unavailable(describeFailure(failure))
    } catch (linkageError: LinkageError) {
        ProviderCredentialLoadOutcome.Unavailable(describeFailure(linkageError))
    }

    /** Removes the stored credential for one provider (both ciphertext and IV). */
    fun clear(providerKey: String) {
        preferences.edit()
            .remove(ciphertextPreferenceKey(providerKey))
            .remove(initializationVectorPreferenceKey(providerKey))
            .commit()
    }

    /** Loads the application's keystore and returns the AES key, or null when absent. */
    private fun existingSecretKey(): SecretKey? {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE_PROVIDER).apply { load(null) }
        return (keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.secretKey
    }

    /** Returns the AES key, generating it inside the Android Keystore on first use. */
    private fun secretKeyOrCreate(): SecretKey {
        existingSecretKey()?.let { return it }
        val keyGenerator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            ANDROID_KEYSTORE_PROVIDER,
        )
        keyGenerator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .setKeySize(KEY_SIZE_BITS)
                .build(),
        )
        return keyGenerator.generateKey()
    }

    /**
     * A reason line for a store failure: the exception's own message (cipher
     * and keystore diagnostics only — never the credential, which this class
     * never passes to an exception).
     */
    private fun describeFailure(failure: Throwable): String =
        failure.message ?: failure::class.simpleName ?: "the device keystore is unavailable"

    private fun encodeBase64(bytes: ByteArray): String = Base64.encodeToString(bytes, Base64.NO_WRAP)

    private fun decodeBase64(text: String): ByteArray = Base64.decode(text, Base64.NO_WRAP)

    private fun ciphertextPreferenceKey(providerKey: String): String =
        "$PREFERENCE_KEY_PREFIX.$providerKey.ciphertext"

    private fun initializationVectorPreferenceKey(providerKey: String): String =
        "$PREFERENCE_KEY_PREFIX.$providerKey.initialization-vector"

    private companion object {
        /** The private preferences file holding only ciphertext and IVs. */
        const val PREFERENCES_NAME: String = "provider-credential-store"

        /** Prefix of the per-provider preference entries. */
        const val PREFERENCE_KEY_PREFIX: String = "provider-credential"

        /** The Android Keystore provider identifier. */
        const val ANDROID_KEYSTORE_PROVIDER: String = "AndroidKeyStore"

        /** The one key alias this application's store uses. */
        const val KEY_ALIAS: String = "provider-credential-key"

        /** AES key size in bits. */
        const val KEY_SIZE_BITS: Int = 256

        /** The authenticated-encryption transformation (GCM, no padding). */
        const val CIPHER_TRANSFORMATION: String = "AES/GCM/NoPadding"

        /** GCM authentication tag length in bits (the standard 128-bit tag). */
        const val AUTHENTICATION_TAG_BITS: Int = 128
    }
}
