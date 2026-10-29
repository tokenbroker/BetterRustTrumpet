//! SHA-256 and ECDSA P-256 signature checks through Windows' CNG (D56:
//! updates are installed only when their signed manifest verifies).
use windows::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_ECCKEY_BLOB, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM,
    BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_FLAGS, BCRYPT_KEY_HANDLE,
    BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS, BCRYPT_SHA256_ALG_HANDLE, BCryptCloseAlgorithmProvider,
    BCryptDestroyKey, BCryptHash, BCryptImportKeyPair, BCryptOpenAlgorithmProvider,
    BCryptVerifySignature,
};
use windows::core::PCWSTR;

/// SHA-256 of `data`.
pub fn sha256(data: &[u8]) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    // SAFETY: the pseudo-handle needs no opening; `out` is the digest size.
    let status = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut out) };
    status.is_ok().then_some(out)
}

/// Whether `signature` (r and s, 32 bytes each) signs SHA-256 `digest`
/// for the P-256 public key `x`, `y`.
pub fn verify_p256(x: &[u8; 32], y: &[u8; 32], digest: &[u8; 32], signature: &[u8; 64]) -> bool {
    let header = BCRYPT_ECCKEY_BLOB {
        dwMagic: BCRYPT_ECDSA_PUBLIC_P256_MAGIC,
        cbKey: 32,
    };
    let mut blob = Vec::with_capacity(8 + 64);
    blob.extend_from_slice(&header.dwMagic.to_le_bytes());
    blob.extend_from_slice(&header.cbKey.to_le_bytes());
    blob.extend_from_slice(x);
    blob.extend_from_slice(y);
    // SAFETY: the provider and key handles are checked before use and
    // released once; every buffer outlives the call that reads it.
    unsafe {
        let mut algorithm = BCRYPT_ALG_HANDLE::default();
        if BCryptOpenAlgorithmProvider(
            &mut algorithm,
            BCRYPT_ECDSA_P256_ALGORITHM,
            PCWSTR::null(),
            BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
        )
        .is_err()
        {
            return false;
        }
        let mut key = BCRYPT_KEY_HANDLE::default();
        let imported =
            BCryptImportKeyPair(algorithm, None, BCRYPT_ECCPUBLIC_BLOB, &mut key, &blob, 0).is_ok();
        let valid = imported
            && BCryptVerifySignature(key, None, digest, signature, BCRYPT_FLAGS(0)).is_ok();
        if imported {
            let _ = BCryptDestroyKey(key);
        }
        let _ = BCryptCloseAlgorithmProvider(algorithm, 0);
        valid
    }
}
