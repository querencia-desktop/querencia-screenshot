// SPDX-License-Identifier: MIT

//! Ausführung externer Programme mit expliziten Argumenten (keine Shell),
//! hinter einer Schnittstelle, die Tests durch Attrappen ersetzen. Die
//! Ausgaben werden beim Lesen begrenzt; ein Prozess, der mehr liefert, wird
//! beendet und eingesammelt, nie unbegrenzt gepuffert.

use std::{
    ffi::OsStr,
    fmt,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

/// Obergrenze für stdout (das PNG eines großen Bildschirms bleibt weit darunter).
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
/// Obergrenze für stderr; Fehlermeldungen sind kurz.
pub const MAX_STDERR_BYTES: usize = 64 * 1024;
/// Wie viel stderr in Meldungen übernommen wird.
const STDERR_EXCERPT_CHARS: usize = 2000;

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
    /// Ausgabe größer als die Grenze; der Prozess wurde beendet.
    Oversized(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(program) => write!(f, "Programm „{program}“ ist nicht installiert"),
            Self::Spawn(program, error) => {
                write!(f, "„{program}“ konnte nicht gestartet werden: {error}")
            }
            Self::Oversized(program) => {
                write!(
                    f,
                    "Ausgabe von „{program}“ ist zu groß; Programm wurde beendet"
                )
            }
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

    /// Wie `run`, mit eigenen Grenzen für stdout und stderr.
    pub fn run_bounded(
        &self,
        program: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
        max_stdout: usize,
        max_stderr: usize,
    ) -> Result<Output, RunError> {
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

        // Jeder Strom bekommt einen eigenen Thread, damit ein voller Puffer
        // auf der einen Seite nie das Lesen der anderen blockiert.
        let writer = child.stdin.take().map(|mut pipe| {
            let bytes = stdin.map(<[u8]>::to_vec).unwrap_or_default();
            thread::spawn(move || {
                // Ein Leser, der vorzeitig schließt, ist kein Fehler des Aufrufers.
                let _ = pipe.write_all(&bytes);
            })
        });
        let stdout = child
            .stdout
            .take()
            .map(|pipe| read_bounded_in_thread(pipe, max_stdout));
        let stderr = child
            .stderr
            .take()
            .map(|pipe| read_bounded_in_thread(pipe, max_stderr));

        let stdout = join_reader(stdout);
        let stderr = join_reader(stderr);
        let overflowed = stdout.overflowed || stderr.overflowed;
        if overflowed {
            // Beenden und einsammeln: kein Zombie, keine weitere Ausgabe.
            let _ = child.kill();
        }
        let status = child.wait().map_err(spawn_error)?;
        if let Some(writer) = writer {
            let _ = writer.join();
        }
        if overflowed {
            return Err(RunError::Oversized(program.to_owned()));
        }
        Ok(Output {
            status: status.code().unwrap_or(-1),
            stdout: stdout.bytes,
            stderr: String::from_utf8_lossy(&stderr.bytes)
                .chars()
                .take(STDERR_EXCERPT_CHARS)
                .collect(),
        })
    }
}

struct BoundedRead {
    bytes: Vec<u8>,
    overflowed: bool,
}

/// Liest höchstens `limit` Bytes; ein weiteres Byte beweist den Überlauf,
/// danach wird nicht mehr gelesen (der Prozess blockiert dann auf dem
/// vollen Rohr, bis `kill` es schließt).
fn read_bounded(mut reader: impl Read, limit: usize) -> BoundedRead {
    let mut bytes = Vec::new();
    let mut probe = reader.by_ref().take(limit as u64 + 1);
    let _ = probe.read_to_end(&mut bytes);
    let overflowed = bytes.len() > limit;
    bytes.truncate(limit);
    BoundedRead { bytes, overflowed }
}

fn read_bounded_in_thread(
    reader: impl Read + Send + 'static,
    limit: usize,
) -> thread::JoinHandle<BoundedRead> {
    thread::spawn(move || read_bounded(reader, limit))
}

fn join_reader(handle: Option<thread::JoinHandle<BoundedRead>>) -> BoundedRead {
    handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or(BoundedRead {
            bytes: Vec::new(),
            overflowed: false,
        })
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

impl CommandRunner for SystemRunner {
    fn run(&self, program: &str, args: &[&str], stdin: Option<&[u8]>) -> Result<Output, RunError> {
        self.run_bounded(program, args, stdin, MAX_OUTPUT_BYTES, MAX_STDERR_BYTES)
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
    use std::time::{Duration, Instant};

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

    #[test]
    fn large_stdin_and_mixed_streams_do_not_deadlock() {
        let runner = SystemRunner;
        // cat liest 1 MiB, während wir stdout lesen: ohne eigene Threads
        // liefe einer der Puffer voll.
        let big = vec![b'x'; 1024 * 1024];
        let output = runner.run("cat", &[], Some(&big)).expect("cat");
        assert_eq!(output.stdout.len(), big.len());
        let output = runner
            .run("sh", &["-c", "printf err >&2; printf out"], None)
            .expect("sh");
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, "err");
    }

    #[test]
    fn oversized_output_stops_the_process_promptly_and_reaps_it() {
        let runner = SystemRunner;
        let before = zombie_children();
        let started = Instant::now();
        // `yes` liefert unendlich viel; ohne Begrenzung käme dieser Test nie zurück.
        let result = runner.run_bounded("yes", &["querencia"], None, 256 * 1024, MAX_STDERR_BYTES);
        assert_eq!(result, Err(RunError::Oversized("yes".into())));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "Begrenzung muss beim Lesen greifen, nicht erst am Ende"
        );
        // Dasselbe für stderr.
        let result = runner.run_bounded(
            "sh",
            &["-c", "yes querencia >&2"],
            None,
            MAX_OUTPUT_BYTES,
            64 * 1024,
        );
        assert_eq!(result, Err(RunError::Oversized("sh".into())));
        assert_eq!(
            zombie_children(),
            before,
            "beendete Kinder sind eingesammelt"
        );
    }

    /// Zombie-Kinder dieses Testprozesses laut /proc.
    fn zombie_children() -> usize {
        let me = std::process::id().to_string();
        std::fs::read_dir("/proc")
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| std::fs::read_to_string(entry.path().join("status")).ok())
                    .filter(|status| {
                        status.contains("State:\tZ")
                            && status
                                .lines()
                                .any(|line| line.strip_prefix("PPid:\t") == Some(me.as_str()))
                    })
                    .count()
            })
            .unwrap_or(0)
    }
}
