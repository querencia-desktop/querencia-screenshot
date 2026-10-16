// SPDX-License-Identifier: MIT

//! Ausführung externer Programme mit expliziten Argumenten (keine Shell),
//! hinter einer Schnittstelle, die Tests durch Attrappen ersetzen. Die
//! Ausgaben werden beim Lesen begrenzt; ein Prozess, der mehr liefert, wird
//! beendet und eingesammelt, nie unbegrenzt gepuffert.

use std::{
    ffi::OsStr,
    fmt, io,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, Command, Stdio},
    sync::mpsc::{self, Sender},
    thread,
};

/// Obergrenze für stdout (das PNG eines großen Bildschirms bleibt weit darunter).
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
/// Obergrenze für stderr; Fehlermeldungen sind kurz.
pub const MAX_STDERR_BYTES: usize = 64 * 1024;
/// Wie viel stderr in Meldungen übernommen wird.
const STDERR_EXCERPT_CHARS: usize = 2000;
/// Lesepuffer je Strom und Schleifendurchlauf.
const READ_CHUNK_BYTES: usize = 64 * 1024;

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
    /// Lesen der Ausgabe fehlgeschlagen; der Prozess wurde beendet.
    Read(String, String),
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
            Self::Read(program, error) => write!(
                f,
                "Ausgabe von „{program}“ konnte nicht gelesen werden ({error}); Programm wurde beendet"
            ),
        }
    }
}

pub trait CommandRunner {
    /// Startet `program` mit `args`, schreibt `stdin` hinein (falls gegeben)
    /// und sammelt die Ausgabe. Programme werden nur über den PATH gefunden.
    fn run(&self, program: &str, args: &[&str], stdin: Option<&[u8]>) -> Result<Output, RunError>;

    /// Für Programme, die nach getaner Arbeit einen Hintergrundprozess
    /// zurücklassen (wl-copy bietet die Zwischenablage weiter an): `stdin`
    /// wird geschrieben, stdout und stderr werden **nicht** als Rohr gehalten
    /// (stderr geht in die eigene Fehlerausgabe, also ins Protokoll), und
    /// gewartet wird nur auf das Ende des gestarteten Prozesses. Mit `run`
    /// hält der Hintergrundprozess stderr offen, und der Aufruf endet erst,
    /// wenn ein anderes Programm die Zwischenablage übernimmt.
    fn run_detaching(&self, program: &str, args: &[&str], stdin: &[u8]) -> Result<i32, RunError>;

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
    ///
    /// Beide Ströme werden parallel in eigenen Threads gelesen und melden
    /// Überlauf, Lesefehler und Ende über einen Channel an diesen Thread.
    /// Beim ersten Überlauf oder Lesefehler wird das Kind sofort beendet und
    /// eingesammelt, ohne auf den anderen Strom zu warten: ein Kind, das nach
    /// dem Überlauf weiterlebt und seine Rohre offen hält, kann diese
    /// Funktion nicht aufhalten.
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

        let writer = child.stdin.take().map(|pipe| {
            let bytes = stdin.map(<[u8]>::to_vec).unwrap_or_default();
            thread::spawn(move || write_all_then_close(pipe, &bytes))
        });
        let (events, inbox) = mpsc::channel();
        let mut pending = 0;
        if let Some(pipe) = child.stdout.take() {
            spawn_reader(Stream::Stdout, pipe, max_stdout, events.clone());
            pending += 1;
        }
        if let Some(pipe) = child.stderr.take() {
            spawn_reader(Stream::Stderr, pipe, max_stderr, events.clone());
            pending += 1;
        }
        drop(events);

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut failure = None;
        while pending > 0 {
            match inbox.recv() {
                Ok(Event::Finished(Stream::Stdout, Ok(bytes))) => stdout = bytes,
                Ok(Event::Finished(Stream::Stderr, Ok(bytes))) => stderr = bytes,
                Ok(Event::Overflow) => {
                    failure = Some(RunError::Oversized(program.to_owned()));
                }
                Ok(Event::Finished(stream, Err(error))) => {
                    failure = Some(RunError::Read(
                        program.to_owned(),
                        format!("{stream}: {error}"),
                    ));
                }
                // Alle Sender weg, obwohl noch Ströme ausstehen: ein Lesethread
                // ist abgebrochen. Das ist ein Fehler, keine leere Ausgabe.
                Err(_) => {
                    failure = Some(RunError::Read(
                        program.to_owned(),
                        "Lesethread wurde unerwartet beendet".to_owned(),
                    ));
                }
            }
            if failure.is_some() {
                break;
            }
            pending -= 1;
        }

        if let Some(error) = failure {
            // Sofort beenden und einsammeln. Auf die verbliebenen Leser und
            // den Schreiber wird nicht gewartet: sie enden, sobald ihre Rohre
            // schließen, und halten weder Kind noch Aufrufer fest.
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }

        // Beide Ströme sind am Ende: das Kind hat seine Ausgabe geschlossen.
        let status = child.wait().map_err(spawn_error)?;
        if let Some(writer) = writer {
            let _ = writer.join();
        }
        Ok(Output {
            status: status.code().unwrap_or(-1),
            stdout,
            stderr: String::from_utf8_lossy(&stderr)
                .chars()
                .take(STDERR_EXCERPT_CHARS)
                .collect(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stream {
    Stdout,
    Stderr,
}

impl fmt::Display for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        })
    }
}

/// Meldung eines Lesethreads an den koordinierenden Thread.
#[derive(Debug)]
enum Event {
    /// Mehr als die erlaubte Menge; der Leser hat aufgehört zu lesen.
    /// Welcher Strom: unerheblich, beide führen zum selben Abbruch.
    Overflow,
    /// Strom zu Ende (Ok) oder Lesefehler (Err).
    Finished(Stream, io::Result<Vec<u8>>),
}

/// Liest `reader` bis zum Ende, höchstens `limit` Bytes. Ein Byte über der
/// Grenze beweist den Überlauf; dann wird `Overflow` gemeldet und nicht
/// weitergelesen. Lesefehler werden gemeldet, nie als leere Ausgabe getarnt.
fn read_stream(stream: Stream, mut reader: impl Read, limit: usize, events: &Sender<Event>) {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    let event = loop {
        match reader.read(&mut chunk) {
            Ok(0) => break Event::Finished(stream, Ok(bytes)),
            Ok(n) => {
                if bytes.len() + n > limit {
                    break Event::Overflow;
                }
                bytes.extend_from_slice(&chunk[..n]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => break Event::Finished(stream, Err(error)),
        }
    };
    // Der Empfänger ist nur weg, wenn der Koordinator schon abgebrochen hat.
    let _ = events.send(event);
}

fn spawn_reader(
    stream: Stream,
    reader: impl Read + Send + 'static,
    limit: usize,
    events: Sender<Event>,
) {
    thread::spawn(move || read_stream(stream, reader, limit, &events));
}

/// Schreibt alles und schließt das Rohr danach ausdrücklich, damit das Kind
/// ein Ende sieht. Ein Kind, das vorher schließt (EPIPE), ist kein Fehler des
/// Aufrufers; das Ergebnis dient nur der Diagnose.
fn write_all_then_close(mut pipe: ChildStdin, bytes: &[u8]) -> io::Result<()> {
    let result = pipe.write_all(bytes).and_then(|()| pipe.flush());
    drop(pipe);
    result
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

    fn run_detaching(&self, program: &str, args: &[&str], stdin: &[u8]) -> Result<i32, RunError> {
        let executable =
            Self::locate(program).ok_or_else(|| RunError::Missing(program.to_owned()))?;
        let spawn_error =
            |error: std::io::Error| RunError::Spawn(program.to_owned(), error.to_string());
        let mut child = Command::new(&executable)
            .args(args.iter().map(OsStr::new))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(spawn_error)?;
        if let Some(pipe) = child.stdin.take() {
            // Ein Kind, das vorzeitig schließt, meldet sich über seinen Status.
            let _ = write_all_then_close(pipe, stdin);
        }
        let status = child.wait().map_err(spawn_error)?;
        Ok(status.code().unwrap_or(-1))
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

        fn run_detaching(
            &self,
            program: &str,
            args: &[&str],
            stdin: &[u8],
        ) -> Result<i32, RunError> {
            self.run(program, args, Some(stdin))
                .map(|output| output.status)
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
    fn detaching_run_does_not_wait_for_a_background_child() {
        // Wie wl-copy: liest stdin, lässt einen Hintergrundprozess mit
        // geerbtem stderr zurück und endet selbst sofort.
        let started = Instant::now();
        let status = SystemRunner
            .run_detaching("sh", &["-c", "cat >/dev/null; sleep 3 & exit 3"], b"daten")
            .expect("run");
        assert_eq!(status, 3);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "wartete auf das Hintergrundkind: {:?}",
            started.elapsed()
        );
        assert_eq!(
            SystemRunner.run_detaching("querencia-gibt-es-nicht", &[], b""),
            Err(RunError::Missing("querencia-gibt-es-nicht".into()))
        );
    }

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
        assert_children_get_reaped();
    }

    #[test]
    fn oversized_output_from_a_lingering_process_returns_promptly() {
        // Das Kind überschreitet die Grenze, lebt danach weiter und hält
        // stdout und stderr offen, ohne noch etwas zu schreiben. Vorher
        // wartete run_bounded auf das Ende von stderr – hier 30 s.
        let runner = SystemRunner;
        for (script, max_stdout, max_stderr) in [
            // Kind selbst lebt weiter (exec: kein Enkel).
            (
                "head -c 300000 /dev/zero; exec sleep 30",
                256 * 1024,
                MAX_STDERR_BYTES,
            ),
            // Überlauf auf stderr.
            (
                "head -c 300000 /dev/zero >&2; exec sleep 30",
                MAX_OUTPUT_BYTES,
                64 * 1024,
            ),
            // Enkel hält die Rohre auch nach dem Beenden des Kindes offen.
            (
                "head -c 300000 /dev/zero; sleep 3",
                256 * 1024,
                MAX_STDERR_BYTES,
            ),
        ] {
            let started = Instant::now();
            let result = runner.run_bounded("sh", &["-c", script], None, max_stdout, max_stderr);
            let elapsed = started.elapsed();
            assert_eq!(result, Err(RunError::Oversized("sh".into())), "{script}");
            assert!(
                elapsed < Duration::from_secs(5),
                "{script}: {elapsed:?} – muss beim Überlauf zurückkehren, nicht beim Prozessende"
            );
        }
        assert_children_get_reaped();
        assert!(
            !has_live_child_named("sleep"),
            "das Kind wurde beendet, nicht nur verlassen"
        );
    }

    #[test]
    fn overflow_is_detected_at_the_boundary_and_errors_are_reported() {
        let (events, inbox) = mpsc::channel();
        read_stream(Stream::Stdout, &b"12345"[..], 5, &events);
        assert!(matches!(
            inbox.recv().expect("event"),
            Event::Finished(Stream::Stdout, Ok(bytes)) if bytes == b"12345"
        ));
        read_stream(Stream::Stdout, &b"123456"[..], 5, &events);
        assert!(matches!(inbox.recv().expect("event"), Event::Overflow));

        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("Rohr defekt"))
            }
        }
        read_stream(Stream::Stderr, Broken, 5, &events);
        assert!(matches!(
            inbox.recv().expect("event"),
            Event::Finished(Stream::Stderr, Err(error)) if error.to_string() == "Rohr defekt"
        ));
        let error = RunError::Read("grim".into(), "stderr: Rohr defekt".into());
        assert_eq!(
            error.to_string(),
            "Ausgabe von „grim“ konnte nicht gelesen werden (stderr: Rohr defekt); Programm wurde beendet"
        );
    }

    /// Lebt noch ein (nicht eingesammeltes, nicht beendetes) Kind dieses
    /// Prozesses mit dem Namen `name`?
    fn has_live_child_named(name: &str) -> bool {
        let me = std::process::id().to_string();
        std::fs::read_dir("/proc")
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| std::fs::read_to_string(entry.path().join("status")).ok())
                    .any(|status| {
                        status.lines().any(|line| line == format!("Name:\t{name}"))
                            && !status.contains("State:\tZ")
                            && status
                                .lines()
                                .any(|line| line.strip_prefix("PPid:\t") == Some(me.as_str()))
                    })
            })
            .unwrap_or(false)
    }

    /// Ein nie eingesammeltes Kind bleibt für immer Zombie; Kinder anderer,
    /// gleichzeitig laufender Tests sind nur für Mikrosekunden Zombies
    /// (zwischen Ende und `wait`). Darum wird kurz gewartet statt ein
    /// Vorher/Nachher-Zählwert verglichen.
    fn assert_children_get_reaped() {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut zombies = zombie_children();
        while zombies > 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
            zombies = zombie_children();
        }
        assert_eq!(zombies, 0, "beendete Kinder sind nicht eingesammelt");
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
