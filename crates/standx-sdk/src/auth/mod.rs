//! Authentication module for StandX API

pub mod credentials;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};

pub use credentials::Credentials;

/// StandX request signer using Ed25519
#[derive(Debug)]
pub struct StandXSigner {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

/// Request signature headers
#[derive(Debug, Clone)]
pub struct RequestSignature {
    pub version: String,
    pub request_id: String,
    pub timestamp: u64,
    pub signature: String,
    pub pubkey: String,
}

impl StandXSigner {
    /// Create a new signer from a Base58-encoded private key
    pub fn from_base58(private_key_base58: &str) -> crate::Result<Self> {
        let private_key_bytes = bs58::decode(private_key_base58).into_vec().map_err(|_e| {
            crate::Error::InvalidCredentials {
                message: "Invalid private key format".to_string(),
            }
        })?;

        let signing_key = SigningKey::from_bytes(&private_key_bytes.try_into().map_err(|_| {
            crate::Error::InvalidCredentials {
                message: "Invalid private key length".to_string(),
            }
        })?);

        let verifying_key = signing_key.verifying_key();

        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    /// Get the public key in hex format
    pub fn pubkey_hex(&self) -> String {
        hex::encode(self.verifying_key.as_bytes())
    }

    /// Sign a request with a caller-provided request ID.
    ///
    /// This is useful for deterministic tests and for protocols that need to
    /// correlate an externally-created UUID with an asynchronous response.
    pub fn sign_request_with_id(
        &self,
        request_id: &str,
        timestamp: u64,
        payload: &str,
    ) -> RequestSignature {
        let version = "v1";
        let message = format!("{version},{request_id},{timestamp},{payload}");

        let signature = self.signing_key.sign(message.as_bytes());

        RequestSignature {
            version: version.to_string(),
            request_id: request_id.to_string(),
            timestamp,
            signature: STANDARD.encode(signature.to_bytes()),
            pubkey: self.pubkey_hex(),
        }
    }

    /// Sign a request with a fresh UUID request ID.
    pub fn sign_request(&self, timestamp: u64, payload: &str) -> RequestSignature {
        let request_id = uuid::Uuid::new_v4().to_string();
        self.sign_request_with_id(&request_id, timestamp, payload)
    }

    /// Sign a request with the current timestamp
    pub fn sign_request_now(&self, payload: &str) -> RequestSignature {
        let timestamp = chrono::Utc::now().timestamp_millis() as u64;
        self.sign_request(timestamp, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signer_from_base58() {
        // Generate a random key for testing
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let private_key_bytes = signing_key.to_bytes();
        let private_key_base58 = bs58::encode(&private_key_bytes).into_string();

        let signer = StandXSigner::from_base58(&private_key_base58).unwrap();

        assert!(!signer.pubkey_hex().is_empty());
    }

    #[test]
    fn test_sign_request() {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let private_key_bytes = signing_key.to_bytes();
        let private_key_base58 = bs58::encode(&private_key_bytes).into_string();

        let signer = StandXSigner::from_base58(&private_key_base58).unwrap();

        let payload = r#"{"symbol":"BTC-USD","side":"buy"}"#;
        let sig = signer.sign_request(1700000000000, payload);

        assert_eq!(sig.version, "v1");
        assert_eq!(sig.timestamp, 1700000000000);
        assert!(!sig.signature.is_empty());
        assert!(!sig.pubkey.is_empty());
        assert!(uuid::Uuid::parse_str(&sig.request_id).is_ok());
    }

    #[test]
    fn test_invalid_base58() {
        let result = StandXSigner::from_base58("invalid!!!");
        assert!(result.is_err());
    }

    #[test]
    fn test_signature_format() {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let private_key_bytes = signing_key.to_bytes();
        let private_key_base58 = bs58::encode(&private_key_bytes).into_string();

        let signer = StandXSigner::from_base58(&private_key_base58).unwrap();

        let payload = r#"{"symbol":"BTC-USD"}"#;
        let sig = signer.sign_request_now(payload);

        // Verify signature is valid base64
        let decoded = STANDARD.decode(&sig.signature);
        assert!(decoded.is_ok());

        // Ed25519 signatures are 64 bytes
        assert_eq!(decoded.unwrap().len(), 64);
    }

    #[test]
    fn test_signature_verification() {
        // Test that signature format is correct and can be decoded
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let private_key_bytes = signing_key.to_bytes();
        let private_key_base58 = bs58::encode(&private_key_bytes).into_string();

        let signer = StandXSigner::from_base58(&private_key_base58).unwrap();

        let payload = r#"{"symbol":"BTC-USD","side":"buy"}"#;
        let sig = signer.sign_request_now(payload);

        // Decode the signature from base64
        let sig_bytes = STANDARD.decode(&sig.signature).unwrap();

        // Decode the public key from hex
        let pubkey_bytes = hex::decode(&sig.pubkey).unwrap();

        // Verify the signature format is correct
        // Ed25519 signatures are 64 bytes
        assert_eq!(sig_bytes.len(), 64);
        // Ed25519 public keys are 32 bytes
        assert_eq!(pubkey_bytes.len(), 32);
    }

    #[test]
    fn test_sign_request_consistency() {
        // Test that signing the same payload produces consistent results
        // Ed25519 is deterministic - same message + same key = same signature
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let private_key_bytes = signing_key.to_bytes();
        let private_key_base58 = bs58::encode(&private_key_bytes).into_string();

        let signer = StandXSigner::from_base58(&private_key_base58).unwrap();

        let payload = r#"{"symbol":"BTC-USD","side":"buy"}"#;
        let timestamp = 1700000000000u64;

        // Sign the same request twice with a fixed correlation ID.
        let request_id = "6f071beb-1b81-4c68-a8bf-66f1589c2146";
        let sig1 = signer.sign_request_with_id(request_id, timestamp, payload);
        let sig2 = signer.sign_request_with_id(request_id, timestamp, payload);

        // Same signer should produce same request_id and pubkey
        assert_eq!(sig1.request_id, sig2.request_id);
        assert_eq!(sig1.pubkey, sig2.pubkey);

        // Ed25519 is deterministic, so signatures should be identical
        assert_eq!(sig1.signature, sig2.signature);
    }

    #[test]
    fn test_sign_request_uses_unique_uuid() {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let private_key_base58 = bs58::encode(signing_key.to_bytes()).into_string();
        let signer = StandXSigner::from_base58(&private_key_base58).unwrap();

        let sig1 = signer.sign_request(1700000000000, "{}");
        let sig2 = signer.sign_request(1700000000000, "{}");

        assert_ne!(sig1.request_id, sig2.request_id);
        assert!(uuid::Uuid::parse_str(&sig1.request_id).is_ok());
        assert!(uuid::Uuid::parse_str(&sig2.request_id).is_ok());
    }
}
