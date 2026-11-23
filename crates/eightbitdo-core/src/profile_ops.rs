//! Active profile switching (`cmd 0x0015`, `WriteSLOT`).
//!
//! The official app sends one payload-less `0x0015` with `subcmd = slot`
//! (`0`..`2`), `total_len = 0`, `offset = 0` (`docs/PROTOCOL_SPEC.md` §6.2);
//! [`commands::switch_slot`] builds exactly that request.
//!
//! Two things remain **unconfirmed on hardware**: whether the controller needs a
//! `0x0006` Save afterwards, and what the acknowledgement looks like. This
//! module therefore sends **no Save**, and it reuses the existing App Mode ACK
//! wait without widening the accepted shapes: a direct `cmd = 0x0015` frame and
//! the `cmd = 0x0004, subcmd = 0x0015` envelope are accepted, unrelated frames
//! are skipped, and a missing ACK fails closed with [`Error::Timeout`] instead
//! of guessing a shape.

use eightbitdo_protocol::commands::{self, cmd};
use eightbitdo_protocol::config_bt2::Slot;

use crate::session::Session;
use crate::transport::Transport;
use crate::Error;

impl<T: Transport> Session<T> {
    /// Switch the controller's active profile slot (`cmd 0x0015`).
    ///
    /// Sends one App Mode `WriteSLOT` request built by
    /// [`commands::switch_slot`] (`cmd = 0x0015`, `subcmd = slot.index()`,
    /// `total_len = chunk_len = 0`, `offset = 0`, `crc16 = 0`) and waits for its
    /// acknowledgement. **No Save (`0x0006`) is sent**: the persistence
    /// requirement of `0x0015` is unconfirmed, so this performs only the switch
    /// the official app performs.
    ///
    /// The ACK wait reuses the existing App Mode convention
    /// (the session's App Mode ACK wait): a direct `cmd = 0x0015` frame and the
    /// `cmd = 0x0004, subcmd = 0x0015` envelope are accepted; input reports and
    /// asynchronous notices are skipped; a `0x000E` error frame is surfaced; and
    /// an envelope naming another command is **not** accepted, so an unknown
    /// response shape ends in [`Error::Timeout`] instead of a false success.
    /// The wait is bounded by the session per-frame timeout
    /// ([`Session::with_timeout`], default [`DEFAULT_TIMEOUT`](crate::DEFAULT_TIMEOUT)).
    ///
    /// The call is recorded as an in-flight write, so a failed or dropped
    /// attempt marks the session for a stale-frame drain before the next read.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] when no matching ACK arrives within the
    /// per-frame timeout, [`Error::UnexpectedResponse`] for a malformed frame or
    /// a controller `0x000E` error, or a transport error (for example
    /// [`Error::Disconnected`]).
    pub async fn switch_profile(&mut self, slot: Slot) -> Result<(), Error> {
        self.start_input_keepalive().await;
        self.prepare_read().await?;
        self.run_write(self.switch_profile_inner(slot)).await
    }

    async fn switch_profile_inner(&self, slot: Slot) -> Result<(), Error> {
        let request = commands::switch_slot(slot);
        // No Save-style subcmd exists for `0x0015`, so the last argument is
        // unused by `ack_matches`; a guessed `subcmd = slot` echo is not
        // accepted because it is not one of the verified shapes.
        self.send_and_await_ack(
            &request,
            false,
            cmd::WRITE_SLOT,
            0,
            self.per_frame_timeout(),
            "WriteSLOT profile switch",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use async_trait::async_trait;
    use eightbitdo_protocol::header::{CustomHeader, PacketKind, HEADER_SIZE};

    use super::*;
    use crate::mock::MockTransport;

    fn switch_frame(slot: Slot) -> Vec<u8> {
        commands::switch_slot(slot)
            .encode_ble(PacketKind::App)
            .expect("the 0x0015 request encodes")
    }

    fn sent_header(frame: &[u8]) -> CustomHeader {
        assert_eq!(frame[0], 0x04, "BLE App Mode prefix");
        CustomHeader::parse(&frame[1..]).expect("sent frame header")
    }

    fn direct_ack(slot: Slot) -> Vec<u8> {
        CustomHeader::new(cmd::WRITE_SLOT, slot.index() as u16, 0, 0)
            .to_bytes()
            .to_vec()
    }

    fn envelope_ack() -> Vec<u8> {
        CustomHeader::new(cmd::RESPONSE, cmd::WRITE_SLOT, 0, 0)
            .to_bytes()
            .to_vec()
    }

    fn input_report() -> Vec<u8> {
        let header = CustomHeader::new(cmd::RESPONSE, cmd::INPUT, 12, 0).with_chunk_len(12);
        let mut frame = header.to_bytes().to_vec();
        frame.extend_from_slice(&[0u8; 12]);
        frame
    }

    /// Transport that records sends and then never answers, so a missing ACK
    /// must end in a real timeout instead of `Disconnected`.
    struct StallingTransport {
        sent: Mutex<Vec<Vec<u8>>>,
    }

    impl StallingTransport {
        fn new() -> Self {
            Self {
                sent: Mutex::new(Vec::new()),
            }
        }

        fn sent(&self) -> Vec<Vec<u8>> {
            self.sent
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    #[async_trait]
    impl Transport for StallingTransport {
        async fn send(&self, data: &[u8]) -> Result<(), Error> {
            self.sent
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(data.to_vec());
            Ok(())
        }

        async fn recv(&self) -> Result<Vec<u8>, Error> {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Err(Error::Disconnected)
        }

        fn mtu(&self) -> u16 {
            64
        }

        async fn disconnect(&self) -> Result<(), Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn sends_the_verified_0x0015_shape_for_every_slot_and_no_save() {
        for slot in Slot::ALL {
            let mock = MockTransport::new();
            mock.push_incoming(&direct_ack(slot));
            let mut session = Session::new(&mock).with_timeout(Duration::from_millis(50));

            session
                .switch_profile(slot)
                .await
                .expect("a direct 0x0015 ACK succeeds");

            let sent = mock.take_sent();
            assert_eq!(sent.len(), 1, "exactly one frame, and no Save");
            assert_eq!(
                sent[0],
                switch_frame(slot),
                "the sent frame is commands::switch_slot"
            );

            let header = sent_header(&sent[0]);
            assert_eq!(header.cmd, cmd::WRITE_SLOT, "cmd = 0x0015");
            assert_eq!(
                header.subcmd,
                slot.index() as u16,
                "subcmd carries the zero-based slot for slot {}",
                slot.number()
            );
            assert_eq!(header.total_len, 0, "total_len = 0");
            assert_eq!(header.offset, 0, "offset = 0");
            assert_eq!(header.chunk_len, 0, "no payload declared");
            assert_eq!(header.crc16, 0, "empty payload leaves crc16 unused");
            assert_eq!(sent[0].len(), 1 + HEADER_SIZE, "header only, no payload");
            assert!(
                !sent.iter().any(|frame| sent_header(frame).cmd == cmd::SAVE),
                "no Save is sent: the persistence requirement is unconfirmed"
            );
        }
    }

    #[tokio::test]
    async fn accepts_the_response_envelope_ack() {
        let mock = MockTransport::new();
        mock.push_incoming(&envelope_ack());
        let mut session = Session::new(&mock).with_timeout(Duration::from_millis(50));

        session
            .switch_profile(Slot::S2)
            .await
            .expect("a cmd=0x0004/subcmd=0x0015 envelope ACK succeeds");
        assert_eq!(mock.sent_frames().len(), 1);
    }

    #[tokio::test]
    async fn skips_an_input_report_before_the_ack() {
        let mock = MockTransport::new();
        mock.push_incoming(&input_report());
        mock.push_incoming(&direct_ack(Slot::S3));
        let mut session = Session::new(&mock).with_timeout(Duration::from_millis(50));

        session
            .switch_profile(Slot::S3)
            .await
            .expect("an input report is skipped, not mistaken for the ACK");
    }

    #[tokio::test]
    async fn times_out_without_an_ack() {
        let transport = StallingTransport::new();
        let mut session = Session::new(&transport).with_timeout(Duration::from_millis(20));

        let result = session.switch_profile(Slot::S1).await;
        assert!(
            matches!(result, Err(Error::Timeout { .. })),
            "a missing ACK must fail closed with a timeout, got {result:?}"
        );
        assert_eq!(transport.sent().len(), 1, "the request was still sent");
        assert_eq!(
            transport.sent()[0],
            switch_frame(Slot::S1),
            "only the switch frame was sent, and no Save"
        );
    }

    #[tokio::test]
    async fn propagates_an_empty_queue_as_disconnect() {
        let mock = MockTransport::new();
        let mut session = Session::new(&mock).with_timeout(Duration::from_millis(20));

        let result = session.switch_profile(Slot::S1).await;
        // The mock queue is empty: `recv` reports `Disconnected` immediately,
        // which is the fail-closed path rather than a fabricated ACK.
        assert!(
            matches!(result, Err(Error::Disconnected)),
            "a closed queue must fail closed, got {result:?}"
        );
        assert_eq!(mock.sent_frames().len(), 1, "the request was still sent");
    }

    #[tokio::test]
    async fn an_unrelated_envelope_is_not_accepted() {
        let mock = MockTransport::new();
        // Another command's ACK envelope, then silence.
        mock.push_incoming(&CustomHeader::new(cmd::RESPONSE, cmd::WRITE_CONFIG, 0, 0).to_bytes());
        let mut session = Session::new(&mock).with_timeout(Duration::from_millis(50));

        let result = session.switch_profile(Slot::S2).await;
        assert!(
            matches!(result, Err(Error::Disconnected | Error::Timeout { .. })),
            "an envelope for another command must not confirm the switch, got {result:?}"
        );
    }

    #[tokio::test]
    async fn propagates_a_missing_link_before_sending() {
        let mock = MockTransport::new();
        mock.disconnect().await.unwrap();
        let mut session = Session::new(&mock).with_timeout(Duration::from_millis(20));

        let result = session.switch_profile(Slot::S1).await;
        assert!(
            matches!(result, Err(Error::Disconnected)),
            "a closed link must fail closed, got {result:?}"
        );
    }

    #[tokio::test]
    async fn surfaces_a_controller_error_frame() {
        let mock = MockTransport::new();
        let mut header = CustomHeader::new(0x000E, 0, 0, 0);
        header.chunk_len = 1;
        mock.push_response(&header, &[0x01]);
        let mut session = Session::new(&mock).with_timeout(Duration::from_millis(50));

        let result = session.switch_profile(Slot::S1).await;
        assert!(
            matches!(result, Err(Error::UnexpectedResponse(ref message)) if message.contains("0x000E")),
            "a controller error must be surfaced, got {result:?}"
        );
    }
}
