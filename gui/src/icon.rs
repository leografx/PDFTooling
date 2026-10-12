//! Window icon (see `iconpix` for the drawing).

use crate::iconpix;

pub fn icon_data(size: u32) -> eframe::egui::IconData {
    eframe::egui::IconData { rgba: iconpix::rgba(size), width: size, height: size }
}

