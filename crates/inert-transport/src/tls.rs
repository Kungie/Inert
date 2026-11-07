//! TLS configuration. TLS 1.3 only, aws-lc-rs provider with the
//! post-quantum hybrid group (X25519MLKEM768) preferred.

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use std::sync::Arc;

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

/// Names of the key exchange groups in preference order (for display/tests).
pub fn kx_groups() -> Vec<String> {
    provider().kx_groups.iter().map(|g| format!("{:?}", g.name())).collect()
}

/// Production client config: WebPKI roots (spec §8 "kanal").
pub fn client_config_webpki() -> Arc<ClientConfig> {
    let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    Arc::new(
        ClientConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .expect("TLS 1.3 supported")
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// Development client config: trusts exactly one dev CA certificate.
pub fn client_config_dev(ca_der: &[u8]) -> Result<Arc<ClientConfig>, String> {
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(ca_der.to_vec())).map_err(|e| e.to_string())?;
    Ok(Arc::new(
        ClientConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ))
}

pub fn server_config(chain: Vec<Vec<u8>>, key_pkcs8_der: Vec<u8>) -> Result<Arc<ServerConfig>, String> {
    let chain = chain.into_iter().map(CertificateDer::from).collect();
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_pkcs8_der));
    Ok(Arc::new(
        ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .map_err(|e| e.to_string())?,
    ))
}

/// A development CA plus a leaf certificate for `host`.
pub struct DevCerts {
    pub ca_der: Vec<u8>,
    pub ca_key_pkcs8: Vec<u8>,
    pub leaf_der: Vec<u8>,
    pub leaf_key_pkcs8: Vec<u8>,
}

/// Creates a dev CA and a leaf for `host`. The CA is only ever trusted by a
/// browser started with that site's dev-trust file.
pub fn generate_dev_certs(host: &str) -> Result<DevCerts, String> {
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose};
    let e = |x: rcgen::Error| x.to_string();

    let ca_key = KeyPair::generate().map_err(e)?;
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).map_err(e)?;
    ca_params.distinguished_name.push(DnType::CommonName, format!("Inert dev CA for {host}"));
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca = ca_params.self_signed(&ca_key).map_err(e)?;

    let leaf_key = KeyPair::generate().map_err(e)?;
    let mut leaf_params = CertificateParams::new(vec![host.to_owned()]).map_err(e)?;
    leaf_params.distinguished_name.push(DnType::CommonName, host);
    let leaf = leaf_params.signed_by(&leaf_key, &ca, &ca_key).map_err(e)?;

    Ok(DevCerts {
        ca_der: ca.der().to_vec(),
        ca_key_pkcs8: ca_key.serialize_der(),
        leaf_der: leaf.der().to_vec(),
        leaf_key_pkcs8: leaf_key.serialize_der(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Role, accept, connect};
    use inert_proto::Message;

    #[test]
    fn post_quantum_hybrid_is_preferred() {
        let groups = kx_groups();
        assert!(groups[0].contains("MLKEM"), "{groups:?}");
    }

    #[tokio::test]
    async fn tls_roundtrip_with_dev_ca() {
        let certs = generate_dev_certs("bank.example").unwrap();
        let server = server_config(vec![certs.leaf_der.clone()], certs.leaf_key_pkcs8.clone()).unwrap();
        let client = client_config_dev(&certs.ca_der).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let acceptor = tokio_rustls::TlsAcceptor::from(server);
        let srv = tokio::spawn(async move {
            let (mut c, _) = accept(&listener, &acceptor).await.unwrap();
            let m = c.recv(false).await.unwrap();
            c.send(&m).await.unwrap();
        });
        let mut c = connect(&addr, "bank.example", client).await.unwrap();
        c.send(&Message::Hello { v: 1 }).await.unwrap();
        assert_eq!(c.recv(false).await.unwrap(), Message::Hello { v: 1 });
        srv.await.unwrap();
        let _ = Role::Client;
    }

    #[tokio::test]
    async fn wrong_host_fails() {
        let certs = generate_dev_certs("bank.example").unwrap();
        let server = server_config(vec![certs.leaf_der.clone()], certs.leaf_key_pkcs8.clone()).unwrap();
        let client = client_config_dev(&certs.ca_der).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let acceptor = tokio_rustls::TlsAcceptor::from(server);
        tokio::spawn(async move {
            let _ = accept(&listener, &acceptor).await;
        });
        assert!(connect(&addr, "evil.example", client).await.is_err());
    }
}
