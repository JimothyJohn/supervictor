// Copyright Claudio Mattera 2024-2025.
//
// Distributed under the MIT License or the Apache 2.0 License at your option.
// See the accompanying files LICENSE-MIT.txt and LICENSE-APACHE-2.0.txt, or
// online at
// https://opensource.org/licenses/MIT
// https://opensource.org/licenses/Apache-2.0

//! HTTP client
//! https://github.com/claudiomattera/esp32c3-embassy/blob/master/esp32c3-embassy/src/http.rs

use embassy_net::dns::DnsSocket;
use embassy_net::dns::Error as DnsError;
use embassy_net::tcp::client::TcpClient;
use embassy_net::tcp::client::TcpClientState;
use embassy_net::tcp::ConnectError as TcpConnectError;
use embassy_net::tcp::Error as TcpError;
use embassy_net::Stack;
use log::debug;

use reqwless::client::HttpClient;
use reqwless::client::TlsConfig;
use reqwless::client::TlsVerify;
use reqwless::request::Method;
use reqwless::Error as ReqwlessError;

use heapless::Vec;

/// Response size
const RESPONSE_SIZE: usize = 4096;

/// HTTP client
///
/// This trait exists to be extended with requests to specific sites, like in
/// [`WorldTimeApiClient`][crate::worldtimeapi::WorldTimeApiClient].
#[allow(async_fn_in_trait)]
pub trait ClientTrait {
    /// Send an HTTP request
    /// TODO remove async because the linter says so?
    async fn send_request(&mut self, url: &str) -> Result<Vec<u8, RESPONSE_SIZE>, Error>;
}

/// HTTP client
pub struct Client {
    /// Wifi stack
    stack: Stack<'static>,

    /// Random numbers generator
    /// Do the cool bit-shifty thing ahead of time and eliminate the wrapper
    seed: u64,

    /// TCP client state
    tcp_client_state: TcpClientState<1, 4096, 4096>,

    /// Buffer for received TLS data
    read_record_buffer: [u8; 16640],

    /// Buffer for transmitted TLS data
    write_record_buffer: [u8; 16640],
}

impl Client {
    /// Create a new client
    pub fn new(stack: Stack<'static>, seed: u64) -> Self {
        debug!("Create TCP client state");
        let tcp_client_state = TcpClientState::<1, 4096, 4096>::new();

        Self {
            stack,
            seed,

            tcp_client_state,

            read_record_buffer: [0_u8; 16640],
            write_record_buffer: [0_u8; 16640],
        }
    }
}

impl ClientTrait for Client {
    async fn send_request(&mut self, url: &str) -> Result<Vec<u8, RESPONSE_SIZE>, Error> {
        debug!("Send HTTPs request to {url}");

        debug!("Create DNS socket");
        let dns_socket = DnsSocket::new(self.stack);

        let tls_config = TlsConfig::new(
            self.seed,
            &mut self.read_record_buffer,
            &mut self.write_record_buffer,
            TlsVerify::None,
        );

        debug!("Create TCP client");
        let tcp_client = TcpClient::new(self.stack, &self.tcp_client_state);

        debug!("Create HTTP client");
        let mut client = HttpClient::new_with_tls(&tcp_client, &dns_socket, tls_config);

        debug!("Create HTTP request");
        let mut buffer = [0_u8; 4096];
        let mut request = client.request(Method::GET, url).await?;

        debug!("Send HTTP request");
        let response = request.send(&mut buffer).await?;

        debug!("Response status: {:?}", response.status);

        let buffer = response.body().read_to_end().await?;

        debug!("Read {} bytes", buffer.len());

        let output =
            Vec::<u8, RESPONSE_SIZE>::from_slice(buffer).map_err(|()| Error::ResponseTooLarge)?;

        Ok(output)
    }
}

/// An error within an HTTP request
#[derive(Debug)]
pub enum Error {
    /// Response was too large
    ResponseTooLarge,

    /// Error within TCP streams
    Tcp(TcpError),

    /// Error within TCP connection
    TcpConnect(#[allow(unused)] TcpConnectError),

    /// Error within DNS system
    Dns(#[allow(unused)] DnsError),

    /// Error in HTTP client
    Reqwless(#[allow(unused)] ReqwlessError),
}

impl From<TcpError> for Error {
    fn from(error: TcpError) -> Self {
        Self::Tcp(error)
    }
}

impl From<TcpConnectError> for Error {
    fn from(error: TcpConnectError) -> Self {
        Self::TcpConnect(error)
    }
}

impl From<DnsError> for Error {
    fn from(error: DnsError) -> Self {
        Self::Dns(error)
    }
}

impl From<ReqwlessError> for Error {
    fn from(error: ReqwlessError) -> Self {
        Self::Reqwless(error)
    }
}
