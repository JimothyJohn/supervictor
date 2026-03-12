/// Certificate generation, verification, and TLS handshake testing.
pub mod certs;
/// Local development pipeline (test, build, serve).
pub mod dev;
/// Build and flash embedded firmware to an ESP32-C3.
pub mod edge;
/// End-to-end device onboarding (certs, server, register, flash, verify).
pub mod onboard;
/// mTLS health-check ping against a remote endpoint.
pub mod ping;
/// Production deployment pipeline (dev + staging gates, then deploy).
pub mod prod;
/// Staging pipeline (deploy to dev stack, integration tests, mTLS check).
pub mod staging;
/// API Gateway mTLS truststore reload via S3 swap.
pub mod truststore;
