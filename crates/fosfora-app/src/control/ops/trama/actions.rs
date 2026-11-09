//! Chain-editor actions the UI and the trama ops share: the layer panel's
//! trama badges call these, and so does `trama/open`.

use crate::trama::CanvasTarget;

impl crate::app::App {
    /// Open the chain editor on the master chain (the Master row's badge).
    pub fn open_trama_on_master(&mut self) {
        self.trama.canvas_target = CanvasTarget::Master;
        self.trama.canvas_open = true;
    }

    /// Select layer `index` and open the chain editor on its chain (a layer
    /// row's badge), not on the master tab it may have been on. `false`, and
    /// nothing changes, when there is no such layer.
    pub fn open_trama_on_layer(&mut self, index: usize) -> bool {
        if index >= self.layer_stack.layers.len() {
            return false;
        }
        self.layer_stack.active_layer = index;
        self.sync_active_layer();
        self.trama.canvas_target = CanvasTarget::SelectedLayer;
        self.trama.canvas_open = true;
        true
    }
}
