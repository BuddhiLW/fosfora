//! The web remote's port, as the Setup page and the `system/web` op set it.

use super::WebSystem;

impl WebSystem {
    /// Serve on another port (saved); restarts the server.
    pub fn set_port(&mut self, port: u16) {
        self.config.port = port;
        self.config.save();
        self.restart_server();
    }
}
