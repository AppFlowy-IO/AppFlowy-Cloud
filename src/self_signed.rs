use rcgen::{Certificate, CertificateParams, KeyPair, RcgenError, SanType};
use secrecy::Secret;

pub const CA_CRT: &str = include_str!("../cert/cert.pem");
pub const CA_KEY: &str = include_str!("../cert/key.pem");

pub fn create_self_signed_certificate() -> Result<(Secret<String>, Secret<String>), RcgenError> {
  let key = KeyPair::from_pem(CA_KEY)?;
  let params = CertificateParams::from_ca_cert_pem(CA_CRT, key)?;
  let ca_cert = Certificate::from_params(params)?;

  let mut params = CertificateParams::default();
  params
    .subject_alt_names
    .push(SanType::IpAddress("127.0.0.1".parse().unwrap()));
  params
    .subject_alt_names
    .push(SanType::IpAddress("0.0.0.0".parse().unwrap()));
  params
    .subject_alt_names
    .push(SanType::DnsName("localhost".to_string()));

  // Generate a certificate that's valid for:
  // 1. localhost
  // 2. 127.0.0.1
  let gen_cert = Certificate::from_params(params)?;
  let server_crt = Secret::new(gen_cert.serialize_pem_with_signer(&ca_cert)?);
  let server_key = Secret::new(gen_cert.serialize_private_key_pem());
  Ok((server_crt, server_key))
}
