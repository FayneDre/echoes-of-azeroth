//! One supervised server process: spawn, output capture, readiness, console,
//! clean stop.

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use rk_core::manifest::StopMethod;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{broadcast, oneshot, watch};

use crate::RuntimeError;

/// Lines of output kept in memory for error messages.
const TAIL_LINES: usize = 60;
/// Default time a `close-stdin` shutdown may stay silent before a kill.
const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(300);
/// How often a shutdown in progress is checked.
const STOP_POLL: Duration = Duration::from_secs(1);

/// When a process counts as started.
#[derive(Debug, Clone)]
pub enum Ready {
    /// A TCP connection to `127.0.0.1:port` succeeds (also for LAN binds).
    Tcp(u16),
    /// A line of output matches.
    LogLine(Regex),
}

/// What to run and how to supervise it.
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub id: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Console output of the current run; the previous run's is kept beside
    /// it as `<name>.previous.log`.
    pub log_file: PathBuf,
    pub ready: Ready,
    pub stop: StopMethod,
    pub start_timeout: Duration,
}

/// Which pipe a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// One line of process output, broadcast to the UI / CLI.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub process: String,
    pub stream: Stream,
    pub text: String,
}

/// How a process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
    /// `None` when killed or ended by a signal.
    pub code: Option<i32>,
}

/// How [`Process::stop`] went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// Exited on its own after the stop request.
    Clean,
    /// Killed: `terminate` method, or the clean shutdown stalled.
    Killed,
    /// It had already ended (crashed, or shut down from its console)
    /// before the stop request.
    AlreadyExited,
}

/// A running (or exited) supervised process.
#[derive(Debug)]
pub struct Process {
    config: ProcessConfig,
    stdin: tokio::sync::Mutex<Option<ChildStdin>>,
    exit: watch::Receiver<Option<Exit>>,
    kill: Mutex<Option<oneshot::Sender<()>>>,
    ready_line: watch::Receiver<bool>,
    tail: Arc<Mutex<VecDeque<String>>>,
    /// When the process last printed a line (a shutdown that still prints
    /// is making progress).
    last_output: Arc<Mutex<Instant>>,
    /// Kills the process when the launcher goes away (`terminate` only).
    #[cfg(windows)]
    _job: Option<win32job::Job>,
}

impl Process {
    /// Spawns the process. Output goes to the log file and `logs`.
    pub async fn spawn(
        config: ProcessConfig,
        logs: broadcast::Sender<LogLine>,
    ) -> Result<Self, RuntimeError> {
        if !config.program.is_file() {
            return Err(RuntimeError::MissingProgram(config.program.clone()));
        }
        if let Ready::Tcp(port) = config.ready {
            // Something already listening would make readiness pass at once.
            if port_open(port).await {
                return Err(RuntimeError::PortInUse {
                    process: config.id.clone(),
                    port,
                });
            }
        }
        let log = open_log(&config.log_file)?;

        let mut command = Command::new(&config.program);
        command
            .args(&config.args)
            .current_dir(&config.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false);
        #[cfg(windows)]
        {
            // No console window, and no Ctrl+C from the launcher's console:
            // stops always go through `stop`.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(RuntimeError::io(&config.program))?;

        #[cfg(windows)]
        let job = match config.stop {
            StopMethod::Terminate => {
                let attach = || -> Result<win32job::Job, String> {
                    let mut limits = win32job::ExtendedLimitInfo::new();
                    limits.limit_kill_on_job_close();
                    let job = win32job::Job::create_with_limit_info(&limits)
                        .map_err(|e| e.to_string())?;
                    let handle = child.raw_handle().ok_or("process already reaped")?;
                    job.assign_process(handle as isize)
                        .map_err(|e| e.to_string())?;
                    Ok(job)
                };
                match attach() {
                    Ok(job) => Some(job),
                    Err(message) => {
                        let _ = child.start_kill();
                        return Err(RuntimeError::Job {
                            process: config.id.clone(),
                            message,
                        });
                    }
                }
            }
            StopMethod::CloseStdin { .. } => None,
        };

        let stdin = child.stdin.take();
        let tail = Arc::new(Mutex::new(VecDeque::with_capacity(TAIL_LINES)));
        let last_output = Arc::new(Mutex::new(Instant::now()));
        let (ready_tx, ready_line) = watch::channel(false);
        let pattern = match &config.ready {
            Ready::LogLine(re) => Some(re.clone()),
            Ready::Tcp(_) => None,
        };
        let log = Arc::new(tokio::sync::Mutex::new(log));
        let reader = |pipe: Option<Box<dyn AsyncRead + Unpin + Send>>, stream| {
            let pump = Pump {
                process: config.id.clone(),
                stream,
                logs: logs.clone(),
                log: log.clone(),
                tail: tail.clone(),
                last_output: last_output.clone(),
                pattern: pattern.clone(),
                ready: ready_tx.clone(),
            };
            pipe.map(|pipe| tokio::spawn(pump.run(pipe)))
        };
        let out = reader(
            child.stdout.take().map(|p| Box::new(p) as _),
            Stream::Stdout,
        );
        let err = reader(
            child.stderr.take().map(|p| Box::new(p) as _),
            Stream::Stderr,
        );

        let (exit_tx, exit) = watch::channel(None);
        let (kill_tx, kill_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let status = tokio::select! {
                status = child.wait() => status,
                _ = kill_rx => {
                    let _ = child.start_kill();
                    child.wait().await
                }
            };
            // Let the readers flush the last lines before reporting the exit.
            for pump in [out, err].into_iter().flatten() {
                let _ = pump.await;
            }
            let code = status.ok().and_then(|s| s.code());
            let _ = exit_tx.send(Some(Exit { code }));
        });

        Ok(Self {
            config,
            stdin: tokio::sync::Mutex::new(stdin),
            exit,
            kill: Mutex::new(Some(kill_tx)),
            ready_line,
            tail,
            last_output,
            #[cfg(windows)]
            _job: job,
        })
    }

    pub fn id(&self) -> &str {
        &self.config.id
    }

    /// True when stdin is a command line (`close-stdin` stop method).
    pub fn takes_console(&self) -> bool {
        matches!(self.config.stop, StopMethod::CloseStdin { .. })
    }

    /// `Some` once the process has exited.
    pub fn exit(&self) -> Option<Exit> {
        *self.exit.borrow()
    }

    /// Waits until the process exits.
    pub async fn wait(&self) -> Exit {
        let mut exit = self.exit.clone();
        loop {
            if let Some(exit) = *exit.borrow_and_update() {
                return exit;
            }
            if exit.changed().await.is_err() {
                return Exit { code: None };
            }
        }
    }

    /// Last lines of output, for error messages.
    pub fn output_tail(&self) -> String {
        let tail = self.tail.lock().unwrap();
        tail.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    /// Waits until the process is ready, fails if it exits first or times out.
    pub async fn wait_ready(&self) -> Result<Duration, RuntimeError> {
        let started = Instant::now();
        loop {
            if let Some(exit) = self.exit() {
                return Err(RuntimeError::ExitedDuringStartup {
                    process: self.config.id.clone(),
                    code: exit.code,
                    output: self.output_tail(),
                });
            }
            let ready = match &self.config.ready {
                Ready::Tcp(port) => port_open(*port).await,
                Ready::LogLine(_) => *self.ready_line.borrow(),
            };
            if ready {
                return Ok(started.elapsed());
            }
            if started.elapsed() > self.config.start_timeout {
                return Err(RuntimeError::StartTimeout {
                    process: self.config.id.clone(),
                    seconds: self.config.start_timeout.as_secs(),
                    output: self.output_tail(),
                });
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Writes one console command (a line) to the process's stdin.
    pub async fn send_line(&self, line: &str) -> Result<(), RuntimeError> {
        let not_running = || RuntimeError::NotRunning {
            process: self.config.id.clone(),
        };
        if self.exit().is_some() {
            return Err(not_running());
        }
        let mut stdin = self.stdin.lock().await;
        let pipe = stdin.as_mut().ok_or_else(not_running)?;
        let mut bytes = line.trim_end_matches(['\r', '\n']).as_bytes().to_vec();
        bytes.push(b'\n');
        let written = async {
            pipe.write_all(&bytes).await?;
            pipe.flush().await
        };
        written.await.map_err(|_| not_running())
    }

    /// Stops the process with its declared method. A `close-stdin`
    /// shutdown is killed only once it has printed nothing for its timeout:
    /// a world server logging out hundreds of bots is still saving.
    pub async fn stop(&self) -> Stopped {
        if self.exit().is_some() {
            return Stopped::AlreadyExited;
        }
        match self.config.stop {
            StopMethod::Terminate => {
                self.kill();
                self.wait().await;
                Stopped::Killed
            }
            StopMethod::CloseStdin { timeout_seconds } => {
                self.stdin.lock().await.take(); // EOF
                let timeout = timeout_seconds
                    .map(Duration::from_secs)
                    .unwrap_or(DEFAULT_STOP_TIMEOUT);
                // Silence counts from the stop request at the earliest.
                *self.last_output.lock().unwrap() = Instant::now();
                loop {
                    if tokio::time::timeout(STOP_POLL, self.wait()).await.is_ok() {
                        return Stopped::Clean;
                    }
                    let silent = self.last_output.lock().unwrap().elapsed();
                    if silent >= timeout {
                        self.kill();
                        self.wait().await;
                        return Stopped::Killed;
                    }
                }
            }
        }
    }

    fn kill(&self) {
        if let Some(kill) = self.kill.lock().unwrap().take() {
            let _ = kill.send(());
        }
    }
}

/// Copies one pipe to the log file, the broadcast and the tail.
struct Pump {
    process: String,
    stream: Stream,
    logs: broadcast::Sender<LogLine>,
    log: Arc<tokio::sync::Mutex<tokio::fs::File>>,
    tail: Arc<Mutex<VecDeque<String>>>,
    last_output: Arc<Mutex<Instant>>,
    pattern: Option<Regex>,
    ready: watch::Sender<bool>,
}

/// Code page 850 for bytes 0x80..=0xFF (box drawing, Western European
/// letters).
#[rustfmt::skip]
const CP850_HIGH: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å',
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', 'ø', '£', 'Ø', '×', 'ƒ',
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '®', '¬', '½', '¼', '¡', '«', '»',
    '░', '▒', '▓', '│', '┤', 'Á', 'Â', 'À', '©', '╣', '║', '╗', '╝', '¢', '¥', '┐',
    '└', '┴', '┬', '├', '─', '┼', 'ã', 'Ã', '╚', '╔', '╩', '╦', '╠', '═', '╬', '¤',
    'ð', 'Ð', 'Ê', 'Ë', 'È', 'ı', 'Í', 'Î', 'Ï', '┘', '┌', '█', '▄', '¦', 'Ì', '▀',
    'Ó', 'ß', 'Ô', 'Ò', 'õ', 'Õ', 'µ', 'þ', 'Þ', 'Ú', 'Û', 'Ù', 'ý', 'Ý', '¯', '´',
    '\u{ad}', '±', '‗', '¾', '¶', '§', '÷', '¸', '°', '¨', '·', '¹', '³', '²', '■', '\u{a0}',
];

/// A line of process output as text: UTF-8 when it is, otherwise the OEM
/// code page of a Windows console. AzerothCore converts its console output
/// to it (`utf8printf` -> `CharToOemBuffW`), which turned its banners into
/// replacement characters when read as UTF-8. Code page 850 is read for
/// it: its box drawing and most accented letters match code page 437.
fn decode_line(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.into(),
        Err(_) => bytes
            .iter()
            .map(|&b| match b {
                0..=0x7f => char::from(b),
                _ => CP850_HIGH[usize::from(b - 0x80)],
            })
            .collect::<String>()
            .into(),
    }
}

impl Pump {
    async fn run(self, pipe: Box<dyn AsyncRead + Unpin + Send>) {
        let mut reader = BufReader::new(pipe);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = decode_line(&buf).trim_end_matches(['\r', '\n']).to_owned();
            {
                let mut log = self.log.lock().await;
                let _ = log.write_all(text.as_bytes()).await;
                let _ = log.write_all(b"\n").await;
            }
            if let Some(re) = &self.pattern {
                if re.is_match(&text) {
                    let _ = self.ready.send(true);
                }
            }
            *self.last_output.lock().unwrap() = Instant::now();
            {
                let mut tail = self.tail.lock().unwrap();
                if tail.len() == TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(text.clone());
            }
            // No subscriber is fine (nobody watching the console).
            let _ = self.logs.send(LogLine {
                process: self.process.clone(),
                stream: self.stream,
                text,
            });
        }
        let _ = self.log.lock().await.flush().await;
    }
}

/// Opens the log file of a new run, keeping the previous run's beside it.
fn open_log(path: &std::path::Path) -> Result<tokio::fs::File, RuntimeError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(RuntimeError::io(dir))?;
    }
    if path.is_file() {
        let previous = path.with_extension("previous.log");
        let _ = std::fs::remove_file(&previous);
        std::fs::rename(path, &previous).map_err(RuntimeError::io(path))?;
    }
    let file = std::fs::File::create(path).map_err(RuntimeError::io(path))?;
    Ok(tokio::fs::File::from_std(file))
}

/// True when something accepts TCP connections on `127.0.0.1:port`.
pub(crate) async fn port_open(port: u16) -> bool {
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    matches!(
        tokio::time::timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(addr)).await,
        Ok(Ok(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oem_output_is_decoded() {
        assert_eq!(decode_line("café ╔".as_bytes()), "café ╔");
        // Code page 850: 0xC9 0xCD 0xBB = the top of a double box, 0x82 = é.
        assert_eq!(decode_line(&[0xC9, 0xCD, 0xBB, b' ', 0x82]), "╔═╗ é");
    }
}
