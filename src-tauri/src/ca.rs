use anyhow::{Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A local certificate authority used to MITM TLS for inspected hosts.
/// The CA cert is persisted so Claude Code can trust it via NODE_EXTRA_CA_CERTS
/// across restarts; per-host leaf certs are minted on demand and cached.
pub struct CaAuthority {
    ca_cert: rcgen::Certificate,
    ca_key: KeyPair,
    cert_pem_path: PathBuf,
    cache: Mutex<HashMap<String, Arc<ServerConfig>>>,
}

impl CaAuthority {
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).ok();
        let cert_pem_path = dir.join("ca.crt");
        let key_pem_path = dir.join("ca.key");

        let (ca_cert, ca_key) = if cert_pem_path.exists() && key_pem_path.exists() {
            let key_pem = std::fs::read_to_string(&key_pem_path).context("read ca.key")?;
            let cert_pem = std::fs::read_to_string(&cert_pem_path).context("read ca.crt")?;
            let ca_key = KeyPair::from_pem(&key_pem).context("parse ca.key")?;
            // Rebuild the issuer with the SAME key pair. Chain validation depends
            // on the key/subject, not the exact bytes, so this matches the
            // persisted ca.crt that Claude Code trusts.
            let params =
                CertificateParams::from_ca_cert_pem(&cert_pem).context("parse ca.crt params")?;
            let ca_cert = params.self_signed(&ca_key).context("rebuild ca cert")?;
            (ca_cert, ca_key)
        } else {
            let ca_key = KeyPair::generate().context("generate ca key")?;
            let mut params = CertificateParams::new(Vec::new()).context("ca params")?;
            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            params
                .distinguished_name
                .push(DnType::CommonName, "ccl-lens Root CA");
            params
                .distinguished_name
                .push(DnType::OrganizationName, "ccl-lens");
            params.key_usages = vec![
                KeyUsagePurpose::KeyCertSign,
                KeyUsagePurpose::CrlSign,
                KeyUsagePurpose::DigitalSignature,
            ];
            let ca_cert = params.self_signed(&ca_key).context("self-sign ca")?;
            std::fs::write(&cert_pem_path, ca_cert.pem()).context("write ca.crt")?;
            std::fs::write(&key_pem_path, ca_key.serialize_pem()).context("write ca.key")?;
            (ca_cert, ca_key)
        };

        Ok(CaAuthority {
            ca_cert,
            ca_key,
            cert_pem_path,
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn ca_cert_path(&self) -> &Path {
        &self.cert_pem_path
    }

    /// A rustls server config presenting a freshly-minted leaf cert for `host`,
    /// signed by this CA. Cached per host. ALPN offers h2 + http/1.1.
    pub fn server_config(&self, host: &str) -> Result<Arc<ServerConfig>> {
        if let Some(cfg) = self.cache.lock().unwrap().get(host) {
            return Ok(cfg.clone());
        }

        let mut params =
            CertificateParams::new(vec![host.to_string()]).context("leaf params")?;
        params.distinguished_name.push(DnType::CommonName, host);

        let leaf_key = KeyPair::generate().context("leaf key")?;
        let leaf = params
            .signed_by(&leaf_key, &self.ca_cert, &self.ca_key)
            .context("sign leaf")?;

        let cert_der: CertificateDer<'static> = leaf.der().clone();
        let key_der: PrivateKeyDer<'static> =
            PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into();

        let mut cfg = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der)
            .context("build server config")?;
        cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        let cfg = Arc::new(cfg);
        self.cache
            .lock()
            .unwrap()
            .insert(host.to_string(), cfg.clone());
        Ok(cfg)
    }
}
