//! The OSC settings the Setup page edits, as one method each: the page and
//! the `bindings/osc-config` / `bindings/osc-send` ops both call these.

use super::OscSystem;

impl OscSystem {
    /// Listen on another UDP port (saved); restarts the listener.
    pub fn set_rx_port(&mut self, port: u16) {
        self.config.rx_port = port;
        self.config.save();
        self.restart_receiver();
    }

    /// Send to another host and port (saved); the sender re-targets at once
    /// when sending is on.
    pub fn set_tx_target(&mut self, host: &str, port: u16) {
        self.config.tx_host = host.to_string();
        self.config.tx_port = port;
        self.config.save();
        if self.config.tx_enabled {
            self.sender
                .configure(&self.config.tx_host, self.config.tx_port);
        }
    }

    /// How many times a second the analysis is sent (saved).
    pub fn set_tx_rate(&mut self, hz: u32) {
        self.config.tx_rate_hz = hz;
        self.config.save();
    }
}
