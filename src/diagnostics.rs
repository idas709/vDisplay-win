use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct LogWriter {
    file: Arc<Mutex<File>>,
    debug_file: Option<Arc<Mutex<File>>>,
}

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
        let res = self.file.lock().map_err(|_| io::Error::other("log lock poisoned"))?.write(buffer)?;
        if let Some(df) = &self.debug_file {
            let _ = df.lock().map(|mut f| f.write_all(buffer));
        }
        Ok(res)
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = self.file.lock().map(|mut f| f.flush());
        if let Some(df) = &self.debug_file {
            let _ = df.lock().map(|mut f| f.flush());
        }
        Ok(())
    }
}

impl Write for DiagnosticWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file.write_all(buffer)?;
        self.file.flush()?;
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
    
    // Also try to open debug.log in the workspace or current directory for immediate inspection
    let workspace_log = PathBuf::from(r"c:\Users\sgt23\Documents\Projects\vDisplay-win\debug.log");
    let debug_file = OpenOptions::new().create(true).append(true).open(&workspace_log)
        .or_else(|_| OpenOptions::new().create(true).append(true).open("debug.log"))
        .ok()
        .map(|f| Arc::new(Mutex::new(f)));

    Ok((LogWriter { file: Arc::new(Mutex::new(file)), debug_file }, path))
}
