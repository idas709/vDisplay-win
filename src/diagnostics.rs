use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct LogWriter(Arc<Mutex<File>>);

pub struct DiagnosticWriter {
    file: LogWriter,
    stderr: io::Stderr,
}

impl DiagnosticWriter {
    pub fn new(file: LogWriter) -> Self {
        Self { file, stderr: io::stderr() }
    }
}

impl Write for LogWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.lock().map_err(|_| io::Error::other("log lock poisoned"))?.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().map_err(|_| io::Error::other("log lock poisoned"))?.flush()
    }
}

impl Write for DiagnosticWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file.write_all(buffer)?;
        self.file.flush()?;
        // A GUI launch normally has no stderr handle. Ignore that side's error
        // while retaining the persistent log; redirected PowerShell launches
        // receive the same bytes through the inherited stderr handle.
        let _ = self.stderr.write_all(buffer);
        let _ = self.stderr.flush();
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()?;
        let _ = self.stderr.flush();
        Ok(())
    }
}

pub fn log_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|root|
        PathBuf::from(root).join("VirtualDisplayWorkspace").join("virtual-display-workspace.log"))
}

pub fn log_writer() -> io::Result<(LogWriter, PathBuf)> {
    let path = log_path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is unavailable"))?;
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    Ok((LogWriter(Arc::new(Mutex::new(file))), path))
}
