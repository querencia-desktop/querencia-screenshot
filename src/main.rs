// SPDX-License-Identifier: MIT

//! Querencia Screenshot: ein kleines klassisches Aufnahmewerkzeug für
//! Wayland/labwc auf Basis von grim, slurp und wl-copy.
//!
//! Ohne Option öffnet sich das Auswahlfenster (eine Instanz, wie bisher).
//! `--screen`, `--window` und `--area` nehmen sofort auf und zeigen danach
//! „Bildschirmfoto speichern“ (GNOME 2: Druck, Alt+Druck); siehe `cli`.

mod capture;
mod cli;
mod clipboard;
mod geometry;
mod instant;
mod runner;
mod screenshot;
mod storage;
mod toplevel;
mod ui;

use gtk::{gio, glib, prelude::*};

pub const APPLICATION_ID: &str = "org.querencia.Screenshot";

fn main() -> glib::ExitCode {
    let mut args = std::env::args_os();
    let program = args
        .next()
        .map(|arg| arg.to_string_lossy().into_owned())
        .unwrap_or_else(|| "querencia-screenshot".to_owned());
    let command = match cli::parse(args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("querencia-screenshot: {message}\n\n{}", cli::USAGE);
            return glib::ExitCode::from(2);
        }
    };
    let application = match command {
        cli::Command::Help => {
            println!("{}", cli::USAGE);
            return glib::ExitCode::SUCCESS;
        }
        cli::Command::Version => {
            println!("querencia-screenshot {}", env!("CARGO_PKG_VERSION"));
            return glib::ExitCode::SUCCESS;
        }
        cli::Command::Gui => {
            let application = gtk::Application::builder()
                .application_id(APPLICATION_ID)
                .build();
            application.connect_activate(ui::present);
            application
        }
        cli::Command::Capture(target) => {
            // Jeder Tastendruck ist eine eigene Aufnahme mit eigenem Dialog,
            // wie bei GNOME 2; eine offene Auswahl-Instanz bleibt unberührt.
            let application = gtk::Application::builder()
                .application_id(APPLICATION_ID)
                .flags(gio::ApplicationFlags::NON_UNIQUE)
                .build();
            application.connect_activate(move |application| instant::start(application, target));
            application
        }
    };
    application.connect_startup(|_| ui::load_styles());
    // Die Optionen sind schon ausgewertet; GApplication bekommt nur den
    // Programmnamen.
    application.run_with_args(&[program])
}
