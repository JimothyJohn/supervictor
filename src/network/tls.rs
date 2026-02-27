use esp_mbedtls::{Certificates, X509};

pub fn load_certificates() -> Certificates<'static> {
    // AI-Generated comment: Load the CA chain certificate data at compile time.
    // AI-Generated comment: The concat! macro appends a null byte, required by X509::pem.
    // AI-Generated comment: as_bytes() gets a reference to the static byte slice.
    let ca_chain_bytes = concat!(include_str!("../../certs/AmazonRootCA1.pem"), "\0").as_bytes();

    // AI-Generated comment: Create the X509 object, borrowing the static data. Returns Option<X509<'static>>.
    let ca_chain = X509::pem(ca_chain_bytes);

    // AI-Generated comment: Load the client certificate data at compile time.
    let client_cert_bytes =
        concat!(include_str!("../../certs/supervictor.cert.pem"), "\0").as_bytes();
    // AI-Generated comment: Create the X509 object, borrowing the static data.
    let client_cert = X509::pem(client_cert_bytes);

    // AI-Generated comment: Load the private key data at compile time.
    let private_key_bytes =
        concat!(include_str!("../../certs/supervictor.key.pem"), "\0").as_bytes();
    // AI-Generated comment: Create the X509 object, borrowing the static data.
    let private_key = X509::pem(private_key_bytes);

    // AI-Generated comment: Construct the Certificates struct.
    // AI-Generated comment: Use .ok() to convert Result<X509, _> to Option<X509>.
    // AI-Generated comment: The resulting Certificates struct has a 'static lifetime.
    Certificates {
        ca_chain: ca_chain.ok(),
        certificate: client_cert.ok(),
        private_key: private_key.ok(),
        password: None, // AI-Generated comment: No password needed for these keys.
    }
}
