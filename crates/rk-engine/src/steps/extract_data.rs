//! `extract-data`: makes an emulator's server data from the player's own
//! client with tools of the server build (ADR 0024), for emulators whose
//! data is not downloadable (`dataExtraction` in the emulator manifest).
//!
//! The tools run one after another in `<target>.partial`, each with that
//! directory as working directory; the client folder is only read. A
//! finished tool leaves `.rk-steps/<id>.done`, so a resumed extraction
//! skips it (the longest tool restarts from scratch when interrupted). Once
//! every tool is done, the intermediate directories are removed, the
//! declared contents are checked and the directory is renamed to `target`:
//! the store never holds a half-made data folder under its final name.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use rk_core::manifest::{ConfigValue, DataExtraction};
use rk_core::template::{expand_str, Variables};
use thiserror::Error;

use crate::error::EngineError;
use crate::{Cancelled, Step, StepContext, StepFuture};

const STEPS_DIR: &str = ".rk-steps";
/// Progress unit: the whole extraction is `PROGRESS_TOTAL`.
const PROGRESS_TOTAL: u64 = 1000;

/// Why an extraction cannot run or did not finish, for the UI.
#[derive(Debug, Error)]
pub enum ExtractDataError {
    /// The tools keep paths in fixed-size buffers.
    #[error("client path {path} is {length} characters long; the extraction tools accept {max}")]
    ClientPathTooLong {
        path: PathBuf,
        length: usize,
        max: usize,
    },
    #[error("the client at {path} has no {locale} data")]
    MissingLocale { path: PathBuf, locale: String },
    #[error("extraction tool {tool} is missing from the server build")]
    MissingTool { tool: PathBuf },
    #[error("{step} exited with {code}; end of {log}:\n{tail}")]
    ToolFailed {
        step: String,
        code: String,
        log: PathBuf,
        tail: String,
    },
    #[error("extraction finished without '{entry}' in {dir}")]
    Incomplete { dir: PathBuf, entry: String },
    #[error("extraction step '{step}': {reason}")]
    Template { step: String, reason: String },
}

pub struct ExtractData {
    pub id: String,
    pub extraction: DataExtraction,
    /// Where the tool paths of the manifest are relative to (an instance root).
    pub tools_root: PathBuf,
    /// The player's game folder.
    pub client_root: PathBuf,
    /// Final data directory (in the shared store layout).
    pub target: PathBuf,
    /// One `<step>.log` per tool.
    pub logs: PathBuf,
    /// Value of `${cpu.count}`.
    pub cpus: usize,
}

impl ExtractData {
    fn work_dir(&self) -> PathBuf {
        let mut name = self
            .target
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".partial");
        self.target.with_file_name(name)
    }

    fn client_data(&self) -> PathBuf {
        self.client_root.join("Data")
    }

    fn variables(&self) -> Variables {
        let mut vars = Variables::new();
        vars.insert(
            "client.root".into(),
            ConfigValue::Text(path_text(&self.client_root)),
        );
        vars.insert(
            "client.data".into(),
            ConfigValue::Text(path_text(&self.client_data())),
        );
        vars.insert(
            "cpu.count".into(),
            ConfigValue::Text(self.cpus.max(1).to_string()),
        );
        vars
    }

    /// Checks the client before hours of work: path length and locale.
    fn check_client(&self) -> Result<(), ExtractDataError> {
        if let Some(max) = self.extraction.max_client_path {
            // The tools get `Data/...` appended: the longest of the two counts.
            let length = path_text(&self.client_data()).chars().count();
            if length > max {
                return Err(ExtractDataError::ClientPathTooLong {
                    path: self.client_root.clone(),
                    length,
                    max,
                });
            }
        }
        if !self.client_data().join(&self.extraction.locale).is_dir() {
            return Err(ExtractDataError::MissingLocale {
                path: self.client_root.clone(),
                locale: self.extraction.locale.clone(),
            });
        }
        Ok(())
    }

    async fn run_tool<C>(
        &self,
        cx: &StepContext<'_, C>,
        work: &Path,
        index: usize,
        vars: &Variables,
    ) -> Result<(), crate::StepError> {
        let step = &self.extraction.steps[index];
        let executable = self.tools_root.join(&step.executable);
        if !executable.is_file() {
            return Err(Box::new(ExtractDataError::MissingTool { tool: executable }));
        }
        let args = step
            .args
            .iter()
            .map(|a| expand_str(a, vars))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ExtractDataError::Template {
                step: step.id.clone(),
                reason: e.to_string(),
            })?;
        for dir in &step.create {
            let dir = work.join(dir);
            std::fs::create_dir_all(&dir).map_err(EngineError::io(&dir))?;
        }
        std::fs::create_dir_all(&self.logs).map_err(EngineError::io(&self.logs))?;
        let log = self.logs.join(format!("{}.log", step.id));
        let open = || {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log)
                .map_err(EngineError::io(&log))
        };
        let (out, err) = (open()?, open()?);
        let mut child = tokio::process::Command::new(&executable)
            .args(&args)
            .current_dir(work)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .kill_on_drop(true)
            .spawn()
            .map_err(EngineError::io(&executable))?;
        let status = loop {
            tokio::select! {
                status = child.wait() => break status.map_err(EngineError::io(&executable))?,
                _ = tokio::time::sleep(Duration::from_millis(500)) => {
                    if cx.is_cancelled() {
                        // Safe: the tool only writes into the work directory,
                        // and an unfinished step runs again on resume.
                        let _ = child.kill().await;
                        return Err(Box::new(Cancelled));
                    }
                }
            }
        };
        // Stopped together with the launcher (Ctrl+C reaches the whole
        // console group): interrupted, not failed; it runs again on resume.
        if !status.success() && cx.is_cancelled() {
            return Err(Box::new(Cancelled));
        }
        if !status.success() {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            let tail = lines[lines.len().saturating_sub(30)..].join("\n");
            return Err(Box::new(ExtractDataError::ToolFailed {
                step: step.id.clone(),
                code: status
                    .code()
                    .map_or_else(|| "a signal".to_owned(), |c| c.to_string()),
                log,
                tail,
            }));
        }
        Ok(())
    }

    async fn extract<C>(&self, cx: &StepContext<'_, C>) -> Result<(), crate::StepError> {
        if self.target.is_dir() {
            return Ok(());
        }
        self.check_client()?;
        let work = self.work_dir();
        let marks = work.join(STEPS_DIR);
        std::fs::create_dir_all(&marks).map_err(EngineError::io(&marks))?;
        let vars = self.variables();
        let weights: f64 = self.extraction.steps.iter().map(|s| s.weight).sum();
        let mut done = 0.0;
        let report = |done: f64| {
            let share = if weights > 0.0 { done / weights } else { 0.0 };
            cx.progress((share * PROGRESS_TOTAL as f64) as u64, Some(PROGRESS_TOTAL));
        };
        for (index, step) in self.extraction.steps.iter().enumerate() {
            let mark = marks.join(format!("{}.done", step.id));
            if !mark.is_file() {
                report(done);
                cx.check_cancelled()?;
                self.run_tool(cx, &work, index, &vars).await?;
                std::fs::write(&mark, b"").map_err(EngineError::io(&mark))?;
            }
            done += step.weight;
        }
        for dir in &self.extraction.remove {
            let dir = work.join(dir);
            if dir.exists() {
                crate::fsutil::remove_dir_all(&dir)?;
            }
        }
        for entry in &self.extraction.contents {
            if !work.join(entry).is_dir() {
                return Err(Box::new(ExtractDataError::Incomplete {
                    dir: work.clone(),
                    entry: entry.clone(),
                }));
            }
        }
        crate::fsutil::remove_dir_all(&marks)?;
        crate::fsutil::rename(&work, &self.target)?;
        report(weights);
        Ok(())
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl<C: Sync> Step<C> for ExtractData {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind(&self) -> &str {
        "extract-data"
    }
    fn fingerprint(&self) -> String {
        format!(
            "{} {:?} from {} with {} -> {}",
            self.extraction.version,
            self.extraction
                .steps
                .iter()
                .map(|s| (&s.id, &s.executable, &s.args, &s.create))
                .collect::<Vec<_>>(),
            self.client_root.display(),
            self.tools_root.display(),
            self.target.display()
        )
    }

    fn run<'a>(&'a self, cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(self.extract(cx))
    }

    /// Drops an unfinished extraction; a finished one is shared data, kept
    /// like a downloaded archive.
    fn undo<'a>(&'a self, _cx: &'a StepContext<'a, C>) -> StepFuture<'a> {
        Box::pin(async move {
            let work = self.work_dir();
            if work.exists() {
                crate::fsutil::remove_dir_all(&work)?;
            }
            Ok(())
        })
    }
}
