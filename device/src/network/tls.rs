use esp_mbedtls::{Certificates, X509};
use esp_println::println;

pub fn load_certificates() -> Certificates<'static> {
    let ca_chain_bytes = concat!(
        include_str!(concat!("../../../", env!("CERT_PATH"), env!("CA_PATH"))),
        "\0"
    )
    .as_bytes();

    let ca_chain = match X509::pem(ca_chain_bytes) {
        Ok(x509) => {
            println!("[TLS] CA chain loaded ({} bytes)", ca_chain_bytes.len());
            Some(x509)
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

    let client_cert_bytes = concat!(
        include_str!(concat!(
            "../../../",
            env!("CERT_PATH"),
            "devices/",
            env!("DEVICE_NAME"),
            "/client.pem"
        )),
        "\0"
    )
    .as_bytes();

    let client_cert = match X509::pem(client_cert_bytes) {
        Ok(x509) => {
            println!(
                "[TLS] Client cert loaded ({} bytes)",
                client_cert_bytes.len()
            );
            Some(x509)
        }
        Err(e) => {
            println!(
                "[TLS] ERROR: Failed to parse client cert (devices/{}.pem): {:?}",
                env!("DEVICE_NAME"),
                e
            );
            None
        }
    };

    let private_key_bytes = concat!(
        include_str!(concat!(
            "../../../",
            env!("CERT_PATH"),
            "devices/",
            env!("DEVICE_NAME"),
            "/client.key"
        )),
        "\0"
    )
    .as_bytes();

    let private_key = match X509::pem(private_key_bytes) {
        Ok(x509) => {
            println!(
                "[TLS] Private key loaded ({} bytes)",
                private_key_bytes.len()
            );
            Some(x509)
        }
        Err(e) => {
            println!(
                "[TLS] ERROR: Failed to parse private key (devices/{}.key): {:?}",
                env!("DEVICE_NAME"),
                e
            );
            None
        }
    };

    Certificates {
        ca_chain: ca_chain,
        certificate: client_cert,
        private_key: private_key,
        password: None,
    }
}
