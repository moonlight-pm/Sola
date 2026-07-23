//! Spawn an ACP agent child and exchange newline-delimited JSON-RPC.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::backend::BackendSpec;

pub struct ChildTransport {
    child: Child,
    stdin: ChildStdin,
    /// Incoming lines from the child's stdout (reader thread).
    lines: Receiver<Result<String, String>>,
}

impl ChildTransport {
    pub fn spawn(spec: &BackendSpec) -> Result<Self, String> {
        if !command_exists(&spec.command) {
            return Err(format!(
                "agent binary not found: {} (install Grok Build or set SOLA_GROK_BIN)",
                spec.command.display()
            ));
        }

        let mut child = Command::new(&spec.command)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", spec.command.display()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "child stdin missing".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "child stdout missing".to_string())?;
        let stderr = child.stderr.take();

        if let Some(stderr) = stderr {
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    if !line.is_empty() {
                        tracing::debug!(target: "sola_agent::grok", "{line}");
                    }
                }
            });
        }

        let (tx, rx) = mpsc::channel();
        spawn_reader(stdout, tx);

        Ok(Self {
            child,
            stdin,
            lines: rx,
        })
    }

    pub fn write_line(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.stdin, "{line}").map_err(|e| format!("write agent stdin: {e}"))?;
        self.stdin
            .flush()
            .map_err(|e| format!("flush agent stdin: {e}"))
    }

    /// Block until the next stdout line, or error if the child closed.
    pub fn read_line(&self) -> Result<String, String> {
        match self.lines.recv() {
            Ok(Ok(line)) => Ok(line),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("agent stdout closed".into()),
        }
    }

    /// Non-blocking poll with timeout via try_recv loop + sleep is handled by caller.
    pub fn try_read_line(&self) -> Option<Result<String, String>> {
        match self.lines.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("agent stdout closed".into()))
            }
        }
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ChildTransport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_reader(stdout: ChildStdout, tx: Sender<Result<String, String>>) {
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if tx.send(Ok(l)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("read agent stdout: {e}")));
                    break;
                }
            }
        }
    });
}

fn command_exists(path: &Path) -> bool {
    if path.is_file() {
        return true;
    }
    // bare name on PATH
    if path.components().count() == 1 {
        if let Ok(p) = std::env::var("PATH") {
            for dir in p.split(':') {
                if Path::new(dir).join(path).is_file() {
                    return true;
                }
            }
        }
    }
    false
}
