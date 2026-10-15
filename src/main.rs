// SPDX-License-Identifier: MIT

//! Querencia Screenshot: ein kleines klassisches Aufnahmewerkzeug für
//! Wayland/labwc auf Basis von grim, slurp und wl-copy.

mod capture;
mod clipboard;
mod geometry;
mod runner;
mod screenshot;
mod storage;
mod ui;

use gtk::prelude::*;

pub const APPLICATION_ID: &str = "org.querencia.Screenshot";

fn main() -> gtk::glib::ExitCode {
    let application = gtk::Application::builder()
        .application_id(APPLICATION_ID)
        .build();
    application.connect_startup(|_| ui::load_styles());
    application.connect_activate(ui::present);
    application.run()
}
