use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct LogWriter(Arc<Mutex<File>>);

impl Write for LogWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.lock().map_err(|_| io::Error::other("log lock poisoned"))?.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().map_err(|_| io::Error::other("log lock poisoned"))?.flush()
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
