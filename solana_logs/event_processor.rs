use anchor_lang::prelude::borsh::BorshDeserialize;
use log::debug;

use super::solana_event_listener::LogsBunch;
use crate::common::solana_logs::parse_logs;

pub trait EventProcessor {
    type Event: anchor_lang::Event + BorshDeserialize;

    fn on_logs(&self, logs_bunch: LogsBunch) {
        let logs = &logs_bunch.logs[..];
        let logs: Vec<&str> = logs.iter().by_ref().map(String::as_str).collect();
        let Ok(events) =
            parse_logs::parse_logs::<Self::Event>(logs.as_slice(), photon::ID.to_string().as_str())
        else {
            log::error!("Failed to parse logs: {:?}", logs);
            return;
        };
        if !events.is_empty() {
            debug!(
                "Logs intercepted, tx_signature: {}, events: {}, need_check: {}",
                logs_bunch.tx_signature.to_string(),
                events.len(),
                logs_bunch.need_check
            );
        }

        for event in events {
            self.on_event(event, &logs_bunch.tx_signature, logs_bunch.slot, logs_bunch.need_check);
        }
    }

    fn on_event(&self, event: Self::Event, signature: &str, slot: u64, need_check: bool);
}
