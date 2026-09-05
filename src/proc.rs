//! Running other programs without blocking the window.
//!
//! Everything goes through `gio::Subprocess` and the GLib main loop: a build
//! takes minutes, and the log has to arrive a line at a time while it does. No
//! threads, no tokio — the brief is explicit about that, and a spinning window
//! during a ten-minute build would be the worst part of the app.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

/// Run something and wait for everything it printed. For short questions —
/// "is flatpak installed", "what runtimes are here" — never for a build.
pub async fn output(argv: &[String]) -> Option<(String, i32)> {
    let process = gio::Subprocess::newv(
        &argv
            .iter()
            .map(std::ffi::OsStr::new)
            .collect::<Vec<_>>(),
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE,
    )
    .ok()?;

    let (stdout, _) = process.communicate_utf8_future(None).await.ok()?;
    let code = process.exit_status();
    Some((stdout.map(|text| text.to_string()).unwrap_or_default(), code))
}

/// True when the command runs and succeeds. Takes the whole command line, not a
/// program name: inside the sandbox a program is reached through
/// `flatpak-spawn --host`, and asking about the wrapper would answer the wrong
/// question.
pub async fn succeeds(argv: &[String]) -> bool {
    output(argv).await.is_some_and(|(_, code)| code == 0)
}

/// A build in progress: the process, so it can be stopped, and nothing else.
#[derive(Debug)]
pub struct Running {
    process: gio::Subprocess,
    stopped: Rc<RefCell<bool>>,
}

impl Running {
    /// Stop it. The build is a child of this process, so this is the only thing
    /// standing between a wrong manifest and a wasted afternoon.
    pub fn cancel(&self) {
        *self.stopped.borrow_mut() = true;
        self.process.force_exit();
    }

    pub fn was_cancelled(&self) -> bool {
        *self.stopped.borrow()
    }
}

/// Start something long, and hand its output back a line at a time.
///
/// `on_line` is called on the main loop for every line, `on_done` once with the
/// exit code — or with `None` when the process could not be started at all,
/// which is a different thing from a build that failed.
pub fn stream(
    argv: &[String],
    on_line: impl Fn(String) + 'static,
    on_done: impl FnOnce(Option<i32>, bool) + 'static,
) -> Option<Rc<Running>> {
    let process = gio::Subprocess::newv(
        &argv
            .iter()
            .map(std::ffi::OsStr::new)
            .collect::<Vec<_>>(),
        // Merged on purpose: flatpak-builder says the interesting things on
        // stderr, and two streams interleaved out of order would be worse than
        // useless in a log someone is trying to read.
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE,
    )
    .ok()?;

    let running = Rc::new(Running {
        process: process.clone(),
        stopped: Rc::new(RefCell::new(false)),
    });

    let stdout = process.stdout_pipe()?;
    let reader = gio::DataInputStream::new(&stdout);

    let running_for_task = running.clone();
    glib::spawn_future_local(async move {
        loop {
            match reader.read_line_utf8_future(glib::Priority::DEFAULT).await {
                Ok(Some(line)) => on_line(line.to_string()),
                // End of output: the process is finishing or gone.
                Ok(None) => break,
                Err(_) => break,
            }
        }

        let code = process
            .wait_future()
            .await
            .ok()
            .map(|_| process.exit_status());
        on_done(code, running_for_task.was_cancelled());
    });

    Some(running)
}
