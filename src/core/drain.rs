//! A channel receiver read by a forwarding task that stops on a token cancel.

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// An `mpsc` receiver read until a token is cancelled or the channel closes.
/// Before the cancel, [`next`](Self::next) yields every received item; the
/// first call that observes the cancel reads the number of items buffered in
/// the receiver, and the calls from then on yield at most that many items,
/// then `None`, whatever the senders send after the cancel.
pub(crate) struct CancelDrain<T> {
    rx: mpsc::Receiver<T>,
    remaining: Option<usize>,
}

impl<T> CancelDrain<T> {
    /// Read `rx`.
    pub(crate) fn new(rx: mpsc::Receiver<T>) -> Self {
        Self {
            rx,
            remaining: None,
        }
    }

    /// The next item to forward. Before `token` is cancelled, the next item
    /// received, or `None` once the channel is closed and empty. After the
    /// cancel, one of the items counted when the cancel was observed, or
    /// `None` once that many have been yielded or the receiver is empty.
    pub(crate) async fn next(&mut self, token: &CancellationToken) -> Option<T> {
        if self.remaining.is_none() {
            let received = tokio::select! {
                biased;
                () = token.cancelled() => None,
                maybe = self.rx.recv() => Some(maybe),
            };
            match received {
                Some(maybe) => return maybe,
                None => self.remaining = Some(self.rx.len()),
            }
        }
        let remaining = self.remaining.as_mut()?;
        if *remaining == 0 {
            return None;
        }
        *remaining -= 1;
        self.rx.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// With 4 items buffered in a 4-slot channel and a producer that keeps
    /// sending after the cancel, `next` yields the 4 buffered items, then
    /// `None`, within 5 s, while the producer is still sending.
    #[tokio::test]
    async fn a_cancelled_drain_stops_while_the_producer_keeps_sending() {
        let (tx, rx) = mpsc::channel::<u32>(4);
        for i in 0..4 {
            tx.send(i).await.expect("buffer an item");
        }
        let producer = tokio::spawn(async move {
            let mut sent = 4u32;
            while tx.send(sent).await.is_ok() {
                sent += 1;
            }
            sent
        });
        let token = CancellationToken::new();
        token.cancel();
        let mut drain = CancelDrain::new(rx);
        let drained = tokio::time::timeout(Duration::from_secs(5), async {
            let mut drained = Vec::new();
            while let Some(item) = drain.next(&token).await {
                drained.push(item);
                tokio::task::yield_now().await;
            }
            drained
        })
        .await;
        let producer_running = !producer.is_finished();
        drop(drain);
        let sent = producer.await.expect("the producer task completes");
        assert!(
            producer_running,
            "the producer stopped before the drain ended; it sent {sent} items"
        );
        assert_eq!(
            drained.as_deref().ok(),
            Some(&[0, 1, 2, 3][..]),
            "items drained after the cancel, within 5 s (Err = timed out); the producer sent {sent}"
        );
    }

    /// Before the cancel, `next` yields each received item, and `None` once
    /// every sender is dropped and the channel is empty.
    #[tokio::test]
    async fn an_uncancelled_drain_reads_until_the_channel_closes() {
        let (tx, rx) = mpsc::channel::<u32>(4);
        for i in 0..3 {
            tx.send(i).await.expect("buffer an item");
        }
        drop(tx);
        let token = CancellationToken::new();
        let mut drain = CancelDrain::new(rx);
        let mut drained = Vec::new();
        while let Some(item) = drain.next(&token).await {
            drained.push(item);
        }
        assert_eq!(drained, vec![0, 1, 2], "items read before the close");
    }
}
