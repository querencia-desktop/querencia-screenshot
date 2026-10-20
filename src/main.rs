// SPDX-License-Identifier: GPL-3.0-or-later

//! Querencia Screenshot: ein kleines klassisches Aufnahmewerkzeug für
//! Wayland/labwc auf Basis von grim, slurp und wl-copy.
//!
//! Ohne Option öffnet sich das Auswahlfenster (eine Instanz). `--screen`,
//! `--window` und `--area` nehmen sofort auf; `--open` öffnet ein PNG.
//! Danach bearbeitet der Editor das Bild (QS-0d); siehe `cli`.

mod annotation;
mod capture;
mod cli;
mod clipboard;
mod editor;
mod geometry;
mod i18n;
mod instant;
mod render;
mod runner;
mod screenshot;
mod storage;
mod toplevel;
mod ui;
mod window_pick;

use gtk::{gio, glib, prelude::*};

use crate::i18n::gettext_f;

pub const APPLICATION_ID: &str = "org.querencia.Screenshot";

fn main() -> glib::ExitCode {
    i18n::init();
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
            // Jeder Tastendruck ist eine eigene Aufnahme mit eigenem Editor;
            // ein offenes Auswahlfenster bleibt unberührt.
            let application = unique_less();
            application.connect_activate(move |application| instant::start(application, target));
            application
        }
        cli::Command::Open(path) => {
            let application = unique_less();
            application.connect_activate(move |application| open_file(application, &path));
            application
        }
    };
    application.connect_startup(|_| ui::load_styles());
    // Die Optionen sind schon ausgewertet; GApplication bekommt nur den
    // Programmnamen.
    application.run_with_args(&[program])
}

fn unique_less() -> gtk::Application {
    gtk::Application::builder()
        .application_id(APPLICATION_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build()
}

/// `--open`: ein vorhandenes PNG im Editor öffnen. Speichern schlägt einen
/// neuen Namen daneben vor („…-markiert.png“), nie die Datei selbst.
fn open_file(application: &gtk::Application, path: &std::path::Path) {
    let loaded = std::fs::read(path)
        .map_err(|error| {
            gettext_f(
                "Cannot read “{path}”: {error}",
                &[
                    ("path", &path.display().to_string()),
                    ("error", &error.to_string()),
                ],
            )
        })
        .and_then(|bytes| {
            screenshot::Png::new(bytes).map_err(|error| {
                gettext_f(
                    // TRANSLATORS: Datei und Grund, z. B. „“a.png”: The capture is not a PNG image“.
                    "“{path}”: {error}",
                    &[
                        ("path", &path.display().to_string()),
                        ("error", &error.to_string()),
                    ],
                )
            })
        });
    match loaded {
        Ok(png) => editor::open(
            application,
            editor::Source {
                png,
                description: gettext_f(
                    // TRANSLATORS: Beschreibung einer geöffneten Datei (Speicherdialog).
                    "File “{name}”",
                    &[(
                        "name",
                        &path.file_name().unwrap_or_default().to_string_lossy(),
                    )],
                ),
                suggested_name: Some(editor::marked_name(path)),
                suggested_folder: path
                    .parent()
                    .filter(|folder| folder.is_dir())
                    .map(std::path::Path::to_path_buf),
            },
        ),
        Err(message) => instant::show_error(application, &message),
    }
}
