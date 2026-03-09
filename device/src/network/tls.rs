use core::ffi::CStr;
use esp_println::println;
use mbedtls_rs::{Certificate, ClientSessionConfig, Credentials, PrivateKey, TlsVersion, X509};

pub fn load_certificates() -> ClientSessionConfig<'static> {
    // CA chain — null-terminated PEM wrapped as CStr
    let ca_chain_pem = concat!(
        include_str!(concat!("../../../", env!("CERT_PATH"), env!("CA_PATH"))),
        "\0"
    );
    let ca_chain_cstr = unsafe { CStr::from_bytes_with_nul_unchecked(ca_chain_pem.as_bytes()) };

    let ca_chain = match Certificate::new(X509::PEM(ca_chain_cstr)) {
        Ok(cert) => {
            println!("[TLS] CA chain loaded ({} bytes)", ca_chain_pem.len());
            Some(cert)
        }
        Err(e) => {
            println!(
                "[TLS] ERROR: Failed to parse CA chain ({}{}): {:?}",
                env!("CERT_PATH"),
                env!("CA_PATH"),
                e
            );
            None
        }
    };

    // Client certificate
    let client_cert_pem = concat!(
        include_str!(concat!(
            "../../../",
            env!("CERT_PATH"),
            "devices/",
            env!("DEVICE_NAME"),
            "/client.pem"
        )),
        "\0"
    );
    let client_cert_cstr =
        unsafe { CStr::from_bytes_with_nul_unchecked(client_cert_pem.as_bytes()) };

    // Client private key
    let client_key_pem = concat!(
        include_str!(concat!(
            "../../../",
            env!("CERT_PATH"),
            "devices/",
            env!("DEVICE_NAME"),
            "/client.key"
        )),
        "\0"
    );
    let client_key_cstr = unsafe { CStr::from_bytes_with_nul_unchecked(client_key_pem.as_bytes()) };

    let creds = match (
        Certificate::new(X509::PEM(client_cert_cstr)),
        PrivateKey::new(X509::PEM(client_key_cstr), None),
    ) {
        (Ok(cert), Ok(key)) => {
            println!(
                "[TLS] Client cert loaded ({} bytes), key loaded ({} bytes)",
                client_cert_pem.len(),
                client_key_pem.len()
            );
            Some(Credentials {
                certificate: cert,
                private_key: key,
            })
        }
        (Err(e), _) => {
            println!(
                "[TLS] ERROR: Failed to parse client cert (devices/{}/client.pem): {:?}",
                env!("DEVICE_NAME"),
                e
            );
            None
        }
        (_, Err(e)) => {
            println!(
                "[TLS] ERROR: Failed to parse client key (devices/{}/client.key): {:?}",
                env!("DEVICE_NAME"),
                e
            );
            None
        }
    };

    // Server name for verification
    let host_cstr = {
        let host_bytes = concat!(env!("HOST"), "\0").as_bytes();
        unsafe { CStr::from_bytes_with_nul_unchecked(host_bytes) }
    };

    ClientSessionConfig {
        ca_chain,
        creds,
        server_name: Some(host_cstr),
        min_version: match option_env!("TLS_VERSION") {
            Some("1.3") => TlsVersion::Tls1_3,
            _ => TlsVersion::Tls1_2,
        },
        ..ClientSessionConfig::new()
    }
}
