use core::net::Ipv4Addr;

/// Common remote endpoints for HTTP connections
pub mod endpoints {
    use super::*;

    /// Default HTTP port
    pub const HTTP_PORT: u16 = 80;

    /// Default HTTPS port
    pub const HTTPS_PORT: u16 = 443;

    /// Default UVICORN port
    pub const UVICORN_PORT: u16 = 8000;

    /// Google server endpoint (IP: 142.250.185.115, Port: 80)
    pub const GOOGLE: (Ipv4Addr, u16) = (Ipv4Addr::new(142, 250, 185, 115), HTTP_PORT);

    /// Default local development server (IP: 10.0.0.31, Port: 8000)
    pub const LOCAL_DEV: (Ipv4Addr, u16) = (Ipv4Addr::new(10, 0, 0, 31), UVICORN_PORT);
}
