//! In-memory [`Transport`] for tests and offline development.
//!
//! The mock records every frame passed to [`MockTransport::send`] and replays
//! frames queued with [`MockTransport::push_incoming`] or
//! [`MockTransport::push_response`]. It performs no framing or protocol
//! validation of its own.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use async_trait::async_trait;
use eightbitdo_protocol::CustomHeader;

use crate::transport::Transport;
use crate::Error;

/// Default link MTU reported by [`MockTransport::new`].
const DEFAULT_MTU: u16 = 64;

struct MockState {
    sent: Vec<Vec<u8>>,
    incoming: VecDeque<Vec<u8>>,
    mtu: u16,
    disconnected: bool,
}

/// A [`Transport`] that keeps everything in memory.
///
/// `send` only records the frame. Tests decide what the device "answers" by
/// queueing notification frames before the session reads them. Notification
/// responses are built **without** the BLE `0x04` prefix, matching what the
/// controller actually sends (`decode_app_response` input).
///
/// ```
/// use eightbitdo_core::{MockTransport, Transport};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let transport = MockTransport::new();
/// transport.push_incoming(&[0u8; 16]);
/// assert_eq!(transport.recv().await.unwrap().len(), 16);
/// assert!(transport.sent_frames().is_empty());
/// # }
/// ```
pub struct MockTransport {
    state: Mutex<MockState>,
}

impl MockTransport {
    /// Create a mock with an empty queue and MTU [`DEFAULT_MTU`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_mtu(DEFAULT_MTU)
    }

    /// Create a mock that reports `mtu` from [`Transport::mtu`].
    #[must_use]
    pub fn with_mtu(mtu: u16) -> Self {
        Self {
            state: Mutex::new(MockState {
                sent: Vec::new(),
                incoming: VecDeque::new(),
                mtu,
                disconnected: false,
            }),
        }
    }

    /// Queue one complete notification frame (header + payload, no prefix).
    pub fn push_incoming(&self, frame: &[u8]) {
        self.lock().incoming.push_back(frame.to_vec());
    }

    /// Queue a response built from `header` and `payload`.
    ///
    /// The mock does not rewrite `chunk_len`; callers construct the header
    /// exactly as the controller would send it.
    pub fn push_response(&self, header: &CustomHeader, payload: &[u8]) {
        let mut frame = header.to_bytes().to_vec();
        frame.extend_from_slice(payload);
        self.push_incoming(&frame);
    }

    /// Snapshot of every frame written with [`Transport::send`].
    #[must_use]
    pub fn sent_frames(&self) -> Vec<Vec<u8>> {
        self.lock().sent.clone()
    }

    /// Drain and return every frame written with [`Transport::send`].
    #[must_use]
    pub fn take_sent(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.lock().sent)
    }

    /// True once [`Transport::disconnect`] has been called.
    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.lock().disconnected
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MockState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transport for MockTransport {
    async fn send(&self, data: &[u8]) -> Result<(), Error> {
        let mut state = self.lock();
        if state.disconnected {
            return Err(Error::Disconnected);
        }
        state.sent.push(data.to_vec());
        Ok(())
    }

    async fn recv(&self) -> Result<Vec<u8>, Error> {
        let mut state = self.lock();
        if state.disconnected {
            return Err(Error::Disconnected);
        }
        state.incoming.pop_front().ok_or(Error::Disconnected)
    }

    fn mtu(&self) -> u16 {
        self.lock().mtu
    }

    async fn disconnect(&self) -> Result<(), Error> {
        self.lock().disconnected = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eightbitdo_protocol::commands;

    #[tokio::test]
    async fn records_sent_frames_and_reports_mtu() {
        let transport = MockTransport::with_mtu(100);
        transport.send(&[0x04, 0xAA]).await.unwrap();
        transport.send(&[0x04, 0xBB]).await.unwrap();

        assert_eq!(transport.mtu(), 100);
        assert_eq!(
            transport.sent_frames(),
            vec![vec![0x04, 0xAA], vec![0x04, 0xBB]]
        );
        assert_eq!(
            transport.take_sent(),
            vec![vec![0x04, 0xAA], vec![0x04, 0xBB]]
        );
        assert!(transport.sent_frames().is_empty());
    }

    #[tokio::test]
    async fn empty_queue_is_disconnected() {
        let transport = MockTransport::new();
        assert!(matches!(transport.recv().await, Err(Error::Disconnected)));
    }

    #[tokio::test]
    async fn push_response_has_no_prefix() {
        let transport = MockTransport::new();
        let request = commands::read_version();
        transport.push_response(&request.header, &[0x11, 0x22]);

        let frame = transport.recv().await.unwrap();
        assert_eq!(frame.len(), eightbitdo_protocol::HEADER_SIZE + 2);
        assert_eq!(&frame[..16], &request.header.to_bytes());
        assert_eq!(&frame[16..], &[0x11, 0x22]);
    }

    #[tokio::test]
    async fn disconnect_blocks_send_and_recv() {
        let transport = MockTransport::new();
        transport.push_incoming(&[0u8; 16]);
        transport.disconnect().await.unwrap();

        assert!(transport.is_disconnected());
        assert!(matches!(
            transport.send(&[0x04]).await,
            Err(Error::Disconnected)
        ));
        assert!(matches!(transport.recv().await, Err(Error::Disconnected)));
    }
}
