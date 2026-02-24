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
    request_id: String,
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
        let private_key_bytes = bs58::decode(private_key_base58)
            .into_vec()
            .map_err(|e| crate::Error::InvalidCredentials)?;

        let signing_key = SigningKey::from_bytes(
            &private_key_bytes
                .try_into()
                .map_err(|_| crate::Error::InvalidCredentials)?,
        );

        let verifying_key = signing_key.verifying_key();
        let request_id = hex::encode(verifying_key.as_bytes());

        Ok(Self {
            signing_key,
            verifying_key,
            request_id,
        })
    }

    /// Get the request ID (hex-encoded public key)
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Get the public key in hex format
    pub fn pubkey_hex(&self) -> String {
        hex::encode(self.verifying_key.as_bytes())
    }

    /// Sign a request
    pub fn sign_request(&self, timestamp: u64, payload: &str) -> RequestSignature {
        let version = "v1";
        let message = format!("{},{},{},{}", version, self.request_id, timestamp, payload);

        let signature = self.signing_key.sign(message.as_bytes());

        RequestSignature {
            version: version.to_string(),
            request_id: self.request_id.clone(),
            timestamp,
            signature: STANDARD.encode(signature.to_bytes()),
            pubkey: self.pubkey_hex(),
        }
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

    // Test private key (Base58) - DO NOT USE IN PRODUCTION
    const TEST_PRIVATE_KEY: &str = "HdsyJD7oWgT7tRZ7L8QJ9K3mP5nQ8rS2vX4wY6zA1bC3dE5fG7hI9jK0lM2nO4pQ6rS8tU0vW1xY2zA3bC4dE5fG6h";

    #[test]
    fn test_signer_from_base58() {
        // Generate a random key for testing
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let private_key_bytes = signing_key.to_bytes();
        let private_key_base58 = bs58::encode(&private_key_bytes).into_string();

        let signer = StandXSigner::from_base58(&private_key_base58).unwrap();

        assert!(!signer.request_id().is_empty());
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
        assert_eq!(sig.request_id, signer.request_id());
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
}
