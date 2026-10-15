//! The details window, run as `pulsar-details.exe` and fed over a pipe so it
//! shows the same history the hover popups draw.

use std::io::{self, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;

use pulsar_core::feed::{FEED_ARG, Message};
use pulsar_core::history::HistoryStore;
use pulsar_core::ipc::{DETAILS_EXE, sibling};
use pulsar_core::metric::{ItemKind, Snapshot};
use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;

/// Snapshots queued for a window that is not reading; beyond this they are dropped.
const BACKLOG: usize = 16;

pub struct Details {
    child: Child,
    lines: SyncSender<String>,
}

/// Writes queued lines to the child until it goes away or the queue closes.
fn write_lines(mut stdin: ChildStdin, lines: Receiver<String>) {
    for line in lines {
        if stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush())
            .is_err()
        {
            return;
        }
    }
}

impl Details {
    pub fn open(history: &HistoryStore, item: Option<ItemKind>) -> io::Result<Self> {
        let exe = sibling(&std::env::current_exe()?, DETAILS_EXE);
        let mut child = Command::new(exe)
            .arg(FEED_ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let (lines, queue) = mpsc::sync_channel(BACKLOG);
        thread::Builder::new()
            .name("pulsar-details-feed".into())
            .spawn(move || write_lines(stdin, queue))?;
        let details = Self { child, lines };
        // The history goes first and may be large; it is never dropped.
        let _ = details.lines.send(Message::history(history).encode());
        details.show(item);
        Ok(details)
    }

    /// Whether the window is still open.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Brings the window forward on the page for `item`.
    pub fn show(&self, item: Option<ItemKind>) {
        // A click gave this process the right to take the foreground; pass it on.
        unsafe {
            let _ = AllowSetForegroundWindow(self.child.id());
        }
        let _ = self.lines.try_send(Message::Show { item }.encode());
    }

    /// False once the window has closed.
    pub fn send(&self, snapshot: &Snapshot) -> bool {
        match self.lines.try_send(Message::snapshot(snapshot).encode()) {
            Ok(()) | Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}
