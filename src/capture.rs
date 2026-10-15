// SPDX-License-Identifier: MIT

//! Aufnahme über grim, Bereichsauswahl über slurp.

use std::fmt;

use crate::{
    geometry::{GeometryError, Region},
    runner::{CommandRunner, RunError},
    screenshot::{Png, PngError},
};

pub const GRIM: &str = "grim";
pub const SLURP: &str = "slurp";

/// Was aufgenommen wird.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Alle Ausgaben zusammen (grim ohne `-g`/`-o`).
    Everything,
    Region(Region),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CaptureError {
    Run(RunError),
    /// grim ist mit Fehler beendet worden; `stderr` ist gekürzt.
    Failed {
        status: i32,
        stderr: String,
    },
    Geometry(GeometryError),
    Image(PngError),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Run(error) => write!(f, "{error}"),
            Self::Failed { status, stderr } if stderr.is_empty() => {
                write!(f, "Die Aufnahme ist fehlgeschlagen (Status {status})")
            }
            Self::Failed { stderr, .. } => {
                write!(f, "Die Aufnahme ist fehlgeschlagen: {}", stderr.trim())
            }
            Self::Geometry(error) => write!(f, "{error}"),
            Self::Image(error) => write!(f, "{error}"),
        }
    }
}

impl From<RunError> for CaptureError {
    fn from(error: RunError) -> Self {
        Self::Run(error)
    }
}

/// Lässt den Benutzer mit slurp einen Bereich aufziehen. `Ok(None)` heißt
/// Abbruch durch den Benutzer (Escape oder Rechtsklick): slurp beendet sich
/// dann mit Status 1 und „selection cancelled“ auf stderr. Jeder andere
/// Fehlstatus ist ein echter Fehler und wird mit stderr gemeldet.
pub fn select_region(runner: &dyn CommandRunner) -> Result<Option<Region>, CaptureError> {
    let output = runner.run(SLURP, &["-f", "%x,%y %wx%h"], None)?;
    if !output.success() {
        if output.status == 1 && slurp_cancelled(&output.stderr) {
            return Ok(None);
        }
        return Err(CaptureError::Failed {
            status: output.status,
            stderr: output.stderr,
        });
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Region::parse(&text)
        .map(Some)
        .map_err(CaptureError::Geometry)
}

fn slurp_cancelled(stderr: &str) -> bool {
    stderr.to_ascii_lowercase().contains("cancel")
}

/// Nimmt `scope` mit grim als PNG auf (Ausgabe auf stdout, keine Datei).
pub fn capture(runner: &dyn CommandRunner, scope: &Scope) -> Result<Png, CaptureError> {
    let geometry;
    let mut args: Vec<&str> = vec!["-t", "png", "-l", "1"];
    if let Scope::Region(region) = scope {
        geometry = region
            .validated()
            .map_err(CaptureError::Geometry)?
            .grim_argument();
        args.extend(["-g", geometry.as_str()]);
    }
    args.push("-");
    let output = runner.run(GRIM, &args, None)?;
    if !output.success() {
        return Err(CaptureError::Failed {
            status: output.status,
            stderr: output.stderr,
        });
    }
    Png::new(output.stdout).map_err(CaptureError::Image)
}

/// Welche Werkzeuge fehlen, damit die Oberfläche das vorab sagen kann.
pub fn missing_tools(runner: &dyn CommandRunner) -> Vec<&'static str> {
    [GRIM, SLURP, crate::clipboard::WL_COPY]
        .into_iter()
        .filter(|tool| !runner.available(tool))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Output, fake::FakeRunner};
    use crate::screenshot::sample;

    fn exit(status: i32, stdout: &[u8], stderr: &str) -> Result<Output, RunError> {
        Ok(Output {
            status,
            stdout: stdout.to_vec(),
            stderr: stderr.to_owned(),
        })
    }

    #[test]
    fn region_selection_parses_slurp_output_with_a_fixed_format() {
        let runner = FakeRunner::default().respond(SLURP, exit(0, b"10,20 300x150\n", ""));
        assert_eq!(
            select_region(&runner).expect("selection"),
            Some(Region {
                x: 10,
                y: 20,
                width: 300,
                height: 150
            })
        );
        assert_eq!(
            runner.calls.borrow()[0].args,
            vec!["-f", "%x,%y %wx%h"],
            "slurp bekommt ein festes Ausgabeformat"
        );
    }

    #[test]
    fn user_cancellation_is_not_an_error() {
        for stderr in ["selection cancelled\n", "Selection cancelled", "cancelled"] {
            let cancelled = FakeRunner::default().respond(SLURP, exit(1, b"", stderr));
            assert_eq!(
                select_region(&cancelled).expect("cancel"),
                None,
                "{stderr:?}"
            );
        }
    }

    #[test]
    fn slurp_failures_are_reported_not_swallowed() {
        let unsupported = FakeRunner::default().respond(
            SLURP,
            exit(1, b"", "compositor doesn't support zwlr_layer_shell_v1\n"),
        );
        assert_eq!(
            select_region(&unsupported),
            Err(CaptureError::Failed {
                status: 1,
                stderr: "compositor doesn't support zwlr_layer_shell_v1\n".into()
            })
        );
        let crashed = FakeRunner::default().respond(SLURP, exit(2, b"", "cancelled by signal"));
        assert!(
            matches!(
                select_region(&crashed),
                Err(CaptureError::Failed { status: 2, .. })
            ),
            "nur Status 1 gilt als Benutzerabbruch"
        );
        let silent = FakeRunner::default().respond(SLURP, exit(1, b"", ""));
        assert!(matches!(
            select_region(&silent),
            Err(CaptureError::Failed { status: 1, .. })
        ));
        let empty_success = FakeRunner::default().respond(SLURP, exit(0, b"", ""));
        assert_eq!(
            select_region(&empty_success),
            Err(CaptureError::Geometry(GeometryError::Empty)),
            "Erfolg ohne Auswahl ist kein stiller Abbruch"
        );
        let garbage = FakeRunner::default().respond(SLURP, exit(0, b"nonsense", ""));
        assert!(matches!(
            select_region(&garbage),
            Err(CaptureError::Geometry(GeometryError::Malformed(_)))
        ));
        let missing = FakeRunner::default().respond(SLURP, Err(RunError::Missing(SLURP.into())));
        assert_eq!(
            select_region(&missing),
            Err(CaptureError::Run(RunError::Missing(SLURP.into())))
        );
    }

    #[test]
    fn capture_passes_the_region_to_grim_and_validates_the_png() {
        let png = sample(300, 150);
        let runner = FakeRunner::default().respond(GRIM, exit(0, &png, ""));
        let region = Region {
            x: 10,
            y: 20,
            width: 300,
            height: 150,
        };
        let image = capture(&runner, &Scope::Region(region)).expect("capture");
        assert_eq!(image.dimensions(), Some((300, 150)));
        assert_eq!(
            runner.calls.borrow()[0].args,
            vec!["-t", "png", "-l", "1", "-g", "10,20 300x150", "-"]
        );

        let runner = FakeRunner::default().respond(GRIM, exit(0, &png, ""));
        capture(&runner, &Scope::Everything).expect("full capture");
        assert_eq!(
            runner.calls.borrow()[0].args,
            vec!["-t", "png", "-l", "1", "-"]
        );
    }

    #[test]
    fn capture_reports_grim_failures_and_bad_images() {
        let failed = FakeRunner::default().respond(
            GRIM,
            exit(
                1,
                b"",
                "compositor doesn't support wlr-screencopy-unstable-v1\n",
            ),
        );
        assert_eq!(
            capture(&failed, &Scope::Everything),
            Err(CaptureError::Failed {
                status: 1,
                stderr: "compositor doesn't support wlr-screencopy-unstable-v1\n".into()
            })
        );
        let empty = FakeRunner::default().respond(GRIM, exit(0, b"", ""));
        assert_eq!(
            capture(&empty, &Scope::Everything),
            Err(CaptureError::Image(PngError::Empty))
        );
        let degenerate = FakeRunner::default();
        assert!(matches!(
            capture(
                &degenerate,
                &Scope::Region(Region {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 5
                })
            ),
            Err(CaptureError::Geometry(GeometryError::Degenerate(_)))
        ));
        assert!(
            degenerate.calls.borrow().is_empty(),
            "grim wird bei ungültiger Geometrie nicht gestartet"
        );
    }

    #[test]
    fn missing_tools_are_listed_by_name() {
        let runner = FakeRunner::default()
            .respond(GRIM, exit(0, b"", ""))
            .respond(SLURP, Err(RunError::Missing(SLURP.into())));
        assert_eq!(
            missing_tools(&runner),
            vec![SLURP, crate::clipboard::WL_COPY]
        );
    }

    #[test]
    fn error_messages_are_german_and_bounded() {
        let error = CaptureError::Failed {
            status: 2,
            stderr: String::new(),
        };
        assert_eq!(
            error.to_string(),
            "Die Aufnahme ist fehlgeschlagen (Status 2)"
        );
        let error = CaptureError::Run(RunError::Missing("slurp".into()));
        assert_eq!(error.to_string(), "Programm „slurp“ ist nicht installiert");
    }
}
