// SPDX-License-Identifier: MIT

//! PNG-Daten in die Wayland-Zwischenablage legen (wl-copy, `image/png`).

use std::fmt;

use crate::{
    runner::{CommandRunner, RunError},
    screenshot::{MIME_TYPE, Png},
};

pub const WL_COPY: &str = "wl-copy";

#[derive(Debug, PartialEq, Eq)]
pub enum ClipboardError {
    Run(RunError),
    Failed { status: i32, stderr: String },
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Run(error) => write!(f, "{error}"),
            Self::Failed { status, stderr } if stderr.is_empty() => {
                write!(f, "Kopieren fehlgeschlagen (Status {status})")
            }
            Self::Failed { stderr, .. } => write!(f, "Kopieren fehlgeschlagen: {}", stderr.trim()),
        }
    }
}

/// Übergibt die Bilddaten binär über stdin; wl-copy bleibt als Anbieter im
/// Hintergrund, bis die Zwischenablage anderweitig belegt wird. Deshalb
/// `run_detaching`: Mit `run` hielte dieser Hintergrundprozess stderr offen
/// und das Kopieren endete nie (QS-1, isoliert nachgewiesen). Meldungen von
/// wl-copy stehen im Protokoll (stderr).
pub fn copy_png(runner: &dyn CommandRunner, png: &Png) -> Result<(), ClipboardError> {
    let status = runner
        .run_detaching(WL_COPY, &["--type", MIME_TYPE], png.bytes())
        .map_err(ClipboardError::Run)?;
    if status == 0 {
        Ok(())
    } else {
        Err(ClipboardError::Failed {
            status,
            stderr: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Output, fake::FakeRunner};
    use crate::screenshot::sample;

    #[test]
    fn wl_copy_receives_the_png_bytes_with_the_image_mime_type() {
        let runner = FakeRunner::default().respond(
            WL_COPY,
            Ok(Output {
                status: 0,
                stdout: Vec::new(),
                stderr: String::new(),
            }),
        );
        let png = Png::new(sample(4, 4)).expect("png");
        copy_png(&runner, &png).expect("copied");
        let calls = runner.calls.borrow();
        assert_eq!(calls[0].program, "wl-copy");
        assert_eq!(calls[0].args, vec!["--type", "image/png"]);
        assert_eq!(
            calls[0].stdin.as_deref(),
            Some(sample(4, 4).as_slice()),
            "binär, kein Base64"
        );
    }

    #[test]
    fn missing_tool_and_failures_are_reported() {
        let png = Png::new(sample(1, 1)).expect("png");
        let missing =
            FakeRunner::default().respond(WL_COPY, Err(RunError::Missing(WL_COPY.into())));
        assert_eq!(
            copy_png(&missing, &png),
            Err(ClipboardError::Run(RunError::Missing(WL_COPY.into())))
        );
        let failed = FakeRunner::default().respond(
            WL_COPY,
            Ok(Output {
                status: 1,
                stdout: Vec::new(),
                stderr: "no wayland display".into(),
            }),
        );
        assert_eq!(
            copy_png(&failed, &png).expect_err("failure").to_string(),
            "Kopieren fehlgeschlagen (Status 1)",
            "stderr von wl-copy steht im Protokoll, nicht in der Meldung"
        );
    }
}
