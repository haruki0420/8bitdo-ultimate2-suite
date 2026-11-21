//! The transport seam between the session logic and a concrete link.
//!
//! Implementations move already-encoded frames. They do not parse headers or
//! interpret commands; that belongs to [`crate::session::Session`].

use async_trait::async_trait;

use crate::Error;

/// A bidirectional, ordered frame link to a controller.
///
/// `send` receives one complete frame to write to the device. `recv` returns
/// one complete notification frame, including the 16-byte header.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Write one encoded frame.
    async fn send(&self, data: &[u8]) -> Result<(), Error>;

    /// Wait for the next notification frame.
    async fn recv(&self) -> Result<Vec<u8>, Error>;

    /// Negotiated link MTU in bytes.
    ///
    /// The protocol's usable payload per write is `mtu - 3`.
    fn mtu(&self) -> u16;

    /// Close the link. Implementations should make this idempotent.
    async fn disconnect(&self) -> Result<(), Error>;
}

/// Allow a `Session` to borrow a transport instead of taking ownership.
#[async_trait]
impl<T: Transport + ?Sized> Transport for &T {
    async fn send(&self, data: &[u8]) -> Result<(), Error> {
        (**self).send(data).await
    }

    async fn recv(&self) -> Result<Vec<u8>, Error> {
        (**self).recv().await
    }

    fn mtu(&self) -> u16 {
        (**self).mtu()
    }

    async fn disconnect(&self) -> Result<(), Error> {
        (**self).disconnect().await
    }
}
