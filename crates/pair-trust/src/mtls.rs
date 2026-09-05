//! Mutual-TLS configuration for the cluster mesh.
//!
//! Confirmed transport profile:
//! * TLS 1.3 only
//! * both sides present a client/server certificate (mutual auth mandatory)
//! * trust decision is **pinning**: a peer is accepted iff its exact certificate
//!   DER is in the pin store (no CA / no name checking) — the pin store already
//!   validated the `urn:nvpair:node` SAN when the peer was pinned.

use crate::identity::Identity;
use crate::pin::PeerPinStore;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    ClientConfig, DigitallySignedStruct, DistinguishedName, ServerConfig, SignatureScheme,
};
use std::sync::{Arc, RwLock};

/// Shared, mutable set of pinned peers.
pub type SharedPins = Arc<RwLock<PeerPinStore>>;

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Verifier that accepts a peer certificate iff its DER is pinned. Used for both
/// directions (server verifying client, client verifying server).
#[derive(Debug)]
struct PinnedPeerVerifier {
    pins: SharedPins,
    provider: Arc<CryptoProvider>,
}

impl PinnedPeerVerifier {
    fn accept(&self, end_entity: &CertificateDer<'_>) -> bool {
        self.pins
            .read()
            .map(|p| p.is_pinned(end_entity.as_ref()))
            .unwrap_or(false)
    }
    fn schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

impl ServerCertVerifier for PinnedPeerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if self.accept(end_entity) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "server certificate not pinned".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        // TLS 1.3 only; 1.2 signatures are never expected.
        Err(rustls::Error::General("TLS 1.2 not supported".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes()
    }
}

impl ClientCertVerifier for PinnedPeerVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        if self.accept(end_entity) {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "client certificate not pinned".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 not supported".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes()
    }
}

fn cert_and_key(id: &Identity) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
    let cert = CertificateDer::from(id.cert_der.clone());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(id.key_pkcs8_der.clone()));
    (vec![cert], key)
}

/// Build a TLS 1.3 server config that requires and pins client certificates.
pub fn server_config(id: &Identity, pins: SharedPins) -> anyhow::Result<ServerConfig> {
    let provider = provider();
    let verifier = Arc::new(PinnedPeerVerifier {
        pins,
        provider: provider.clone(),
    });
    let (chain, key) = cert_and_key(id);
    let cfg = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(verifier)
        .with_single_cert(chain, key)?;
    Ok(cfg)
}

/// Build a TLS 1.3 client config that presents our certificate and pins the
/// server certificate.
pub fn client_config(id: &Identity, pins: SharedPins) -> anyhow::Result<ClientConfig> {
    let provider = provider();
    let verifier = Arc::new(PinnedPeerVerifier {
        pins,
        provider: provider.clone(),
    });
    let (chain, key) = cert_and_key(id);
    let cfg = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(chain, key)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_mutually_pinned_configs() {
        let server = Identity::generate().unwrap();
        let client = Identity::generate().unwrap();

        // Each side pins the other.
        let server_pins = Arc::new(RwLock::new(PeerPinStore::new()));
        server_pins.write().unwrap().pin(&client.cert_der).unwrap();
        let client_pins = Arc::new(RwLock::new(PeerPinStore::new()));
        client_pins.write().unwrap().pin(&server.cert_der).unwrap();

        assert!(server_config(&server, server_pins).is_ok());
        assert!(client_config(&client, client_pins).is_ok());
    }
}
