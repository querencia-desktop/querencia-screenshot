// SPDX-License-Identifier: MIT

//! Ausführung externer Programme mit expliziten Argumenten (keine Shell),
//! hinter einer Schnittstelle, die Tests durch Attrappen ersetzen.

use std::{
    ffi::OsStr,
    fmt,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Obergrenze für gelesene Programmausgaben (PNG eines großen Bildschirms
/// bleibt weit darunter).
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub status: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.status == 0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunError {
    /// Programm nicht im PATH.
    Missing(String),
    /// Start oder Wartezeit fehlgeschlagen.
    Spawn(String, String),
    /// Ausgabe größer als `MAX_OUTPUT_BYTES`.
    Oversized(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(program) => write!(f, "Programm „{program}“ ist nicht installiert"),
            Self::Spawn(program, error) => {
                write!(f, "„{program}“ konnte nicht gestartet werden: {error}")
            }
            Self::Oversized(program) => write!(f, "Ausgabe von „{program}“ ist zu groß"),
        }
    }
}

pub trait CommandRunner {
    /// Startet `program` mit `args`, schreibt `stdin` hinein (falls gegeben)
    /// und sammelt die Ausgabe. Programme werden nur über den PATH gefunden.
    fn run(&self, program: &str, args: &[&str], stdin: Option<&[u8]>) -> Result<Output, RunError>;

    fn available(&self, program: &str) -> bool;
}

/// Ausführung über `std::process::Command`.
#[derive(Default)]
pub struct SystemRunner;

impl SystemRunner {
    fn locate(program: &str) -> Option<PathBuf> {
        if program.contains('/') {
            return None;
        }
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(program))
                .find(|candidate| is_executable(candidate))
        })
    }
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str], stdin: Option<&[u8]>) -> Result<Output, RunError> {
        let executable =
            Self::locate(program).ok_or_else(|| RunError::Missing(program.to_owned()))?;
        let spawn_error =
            |error: std::io::Error| RunError::Spawn(program.to_owned(), error.to_string());
        let mut command = Command::new(&executable);
        command
            .args(args.iter().map(OsStr::new))
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(spawn_error)?;
        if let Some(bytes) = stdin
            && let Some(mut pipe) = child.stdin.take()
        {
            // Ein Leser, der vorzeitig schließt, ist kein Fehler des Aufrufers.
            let _ = pipe.write_all(bytes);
        }
        let output = child.wait_with_output().map_err(spawn_error)?;
        if output.stdout.len() > MAX_OUTPUT_BYTES {
            return Err(RunError::Oversized(program.to_owned()));
        }
        Ok(Output {
            status: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(2000)
                .collect(),
        })
    }

    fn available(&self, program: &str) -> bool {
        Self::locate(program).is_some()
    }
}

#[cfg(test)]
pub mod fake {
    //! Attrappe für Tests: vorbereitete Antworten je Programm, Aufzeichnung
    //! der tatsächlichen Aufrufe.

    use std::cell::RefCell;

    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Call {
        pub program: String,
        pub args: Vec<String>,
        pub stdin: Option<Vec<u8>>,
    }

    #[derive(Default)]
    pub struct FakeRunner {
        responses: RefCell<Vec<(String, Result<Output, RunError>)>>,
        pub calls: RefCell<Vec<Call>>,
    }

    impl FakeRunner {
        pub fn respond(self, program: &str, response: Result<Output, RunError>) -> Self {
            self.responses
                .borrow_mut()
                .push((program.to_owned(), response));
            self
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(
            &self,
            program: &str,
            args: &[&str],
            stdin: Option<&[u8]>,
        ) -> Result<Output, RunError> {
            self.calls.borrow_mut().push(Call {
                program: program.to_owned(),
                args: args.iter().map(|arg| (*arg).to_owned()).collect(),
                stdin: stdin.map(<[u8]>::to_vec),
            });
            let mut responses = self.responses.borrow_mut();
            let position = responses
                .iter()
                .position(|(name, _)| name == program)
                .unwrap_or_else(|| panic!("keine Antwort für {program} vorbereitet"));
            responses.remove(position).1
        }

        fn available(&self, program: &str) -> bool {
            self.responses.borrow().iter().any(|(name, response)| {
                name == program && !matches!(response, Err(RunError::Missing(_)))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_programs_from_the_path_with_explicit_arguments() {
        let runner = SystemRunner;
        let output = runner
            .run("printf", &["%s|%s", "a b", "$HOME"], None)
            .expect("printf");
        assert!(output.success());
        assert_eq!(output.stdout, b"a b|$HOME", "keine Shell-Interpolation");
    }

    #[test]
    fn missing_programs_and_paths_are_refused() {
        let runner = SystemRunner;
        assert_eq!(
            runner.run("querencia-no-such-tool", &[], None),
            Err(RunError::Missing("querencia-no-such-tool".into()))
        );
        assert_eq!(
            runner.run("/bin/true", &[], None),
            Err(RunError::Missing("/bin/true".into())),
            "absolute Pfade werden nicht akzeptiert"
        );
        assert!(!runner.available("querencia-no-such-tool"));
        assert!(runner.available("printf"));
    }

    #[test]
    fn stdin_reaches_the_program_and_failures_keep_their_status() {
        let runner = SystemRunner;
        let output = runner.run("cat", &[], Some(b"png-bytes")).expect("cat");
        assert_eq!(output.stdout, b"png-bytes");
        let failed = runner.run("false", &[], None).expect("false");
        assert!(!failed.success());
        assert_eq!(failed.status, 1);
    }
}
