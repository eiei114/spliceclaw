use std::path::Path;
#[cfg(any(target_os = "macos", test))]
use std::time::Duration;

use crate::error::AppError;

// No cookie polling while Firefox holds the database, no database copy, and no
// force-kill fallback. Auth status reads the session after this cycle finishes.
#[cfg(any(target_os = "macos", test))]
trait Browser {
    fn ensure_closed(&mut self) -> Result<(), AppError>;
    fn open(&mut self, profile: &Path) -> Result<(), AppError>;
    fn quit(&mut self) -> Result<(), AppError>;
}

#[cfg(any(target_os = "macos", test))]
fn cycle(
    browser: &mut impl Browser,
    profile: &Path,
    wait: Duration,
    pause: impl FnOnce(Duration),
) -> Result<(), AppError> {
    browser.ensure_closed()?;
    browser.open(profile)?;
    eprintln!(
        "Firefox opened at splice.com/sounds. Sign in if needed; it will quit normally after {} seconds. Do not use other Firefox windows during this command.",
        wait.as_secs()
    );
    pause(wait);
    browser.quit()
}

pub(crate) fn refresh(
    root: &Path,
    profile: Option<&str>,
    wait_seconds: u64,
) -> Result<String, AppError> {
    #[cfg(target_os = "macos")]
    {
        let (directory, name) = crate::firefox::profile_dir(root, profile)?;
        if !directory.is_dir() {
            return Err(AppError::new(
                "firefox_profile_missing",
                "The selected Firefox profile directory does not exist.",
                "Open Firefox once to create the profile, then retry.",
            ));
        }
        let executable = crate::firefox::executable_candidates("macos", |key| {
            std::env::var_os(key).map(std::path::PathBuf::from)
        })
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            AppError::new(
                "firefox_missing",
                "Firefox is not installed in a supported location.",
                "Install Firefox in /Applications or ~/Applications.",
            )
        })?;
        let mut browser = macos::Firefox {
            executable,
            child: None,
        };
        cycle(
            &mut browser,
            &directory,
            Duration::from_secs(wait_seconds),
            std::thread::sleep,
        )?;
        Ok(name)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (root, profile, wait_seconds);
        Err(AppError::new(
            "refresh_unsupported",
            "Automatic Firefox refresh currently supports macOS only.",
            "Open Splice in Firefox, sign in, quit Firefox, then run auth status --live.",
        ))
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use crate::error::AppError;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    pub(super) struct Firefox {
        pub executable: PathBuf,
        pub child: Option<Child>,
    }

    fn control_error() -> AppError {
        AppError::new(
            "firefox_control_failed",
            "Cannot safely control the Firefox process.",
            "Close Firefox manually and run auth status --live. No Firefox process was force-killed.",
        )
    }

    fn pids() -> Result<Vec<u32>, AppError> {
        let output = Command::new("/usr/bin/pgrep")
            .args(["-x", "firefox"])
            .stderr(Stdio::null())
            .output()
            .map_err(|_| control_error())?;
        if output.status.code() == Some(1) {
            return Ok(Vec::new());
        }
        if !output.status.success() {
            return Err(control_error());
        }
        let text = std::str::from_utf8(&output.stdout).map_err(|_| control_error())?;
        text.lines()
            .map(|line| line.parse().map_err(|_| control_error()))
            .collect()
    }

    // Bound both the AppleEvent and the actual browser exit. Kill only a stuck
    // osascript helper, never the browser, to avoid losing the refreshed session.
    fn wait_for_exit(child: &mut Child, timeout: Duration) -> Result<bool, AppError> {
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait().map_err(|_| control_error())? {
                return Ok(status.success());
            }
            if start.elapsed() >= timeout {
                return Err(AppError::new(
                    "firefox_quit_timeout",
                    "Firefox did not complete normal shutdown in time.",
                    "Handle any Firefox dialog, quit Firefox manually, then run auth status --live. No browser was force-killed.",
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    impl super::Browser for Firefox {
        fn ensure_closed(&mut self) -> Result<(), AppError> {
            if !pids()?.is_empty() {
                return Err(AppError::new(
                    "firefox_already_running",
                    "Firefox is already running. It was not closed or changed.",
                    "Quit Firefox first. This command only manages Firefox that it starts itself.",
                ));
            }
            Ok(())
        }

        fn open(&mut self, profile: &Path) -> Result<(), AppError> {
            self.child = Some(
                Command::new(&self.executable)
                    .arg("-no-remote")
                    .arg("-profile")
                    .arg(profile)
                    .args(["-new-window", "https://splice.com/sounds"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|_| control_error())?,
            );
            Ok(())
        }

        fn quit(&mut self) -> Result<(), AppError> {
            let child = self.child.as_mut().ok_or_else(control_error)?;
            if child.try_wait().map_err(|_| control_error())?.is_some() {
                return Err(AppError::new(
                    "firefox_exited_early",
                    "Firefox exited before the refresh wait finished.",
                    "Retry with Firefox closed. Leave the window open until the command closes it.",
                ));
            }
            // Do not send a quit AppleEvent if another browser instance appeared.
            if pids()? != [child.id()] {
                return Err(control_error());
            }
            let mut quit = Command::new("/usr/bin/osascript")
                .args(["-e", "tell application id \"org.mozilla.firefox\" to quit"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| control_error())?;
            match wait_for_exit(&mut quit, Duration::from_secs(10)) {
                Ok(true) => (),
                Ok(false) => return Err(control_error()),
                Err(error) => {
                    let _ = quit.kill();
                    let _ = quit.wait();
                    return Err(error);
                }
            }
            if !wait_for_exit(child, Duration::from_secs(15))? {
                return Err(control_error());
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        events: Vec<&'static str>,
        fail: Option<&'static str>,
    }
    impl Fake {
        fn event(&mut self, event: &'static str) -> Result<(), AppError> {
            self.events.push(event);
            if self.fail == Some(event) {
                return Err(AppError::new("test", "test", "test"));
            }
            Ok(())
        }
    }
    impl Browser for Fake {
        fn ensure_closed(&mut self) -> Result<(), AppError> {
            self.event("check")
        }
        fn open(&mut self, _: &Path) -> Result<(), AppError> {
            self.event("open")
        }
        fn quit(&mut self) -> Result<(), AppError> {
            self.event("quit")
        }
    }

    #[test]
    fn cycle_waits_then_closes_before_returning() {
        let mut browser = Fake::default();
        let mut waited = false;
        cycle(
            &mut browser,
            Path::new("/profile"),
            Duration::from_secs(20),
            |duration| {
                assert_eq!(duration.as_secs(), 20);
                waited = true;
            },
        )
        .unwrap();
        assert!(waited);
        assert_eq!(browser.events, ["check", "open", "quit"]);
    }

    #[test]
    fn running_browser_is_never_opened_or_quit() {
        let mut browser = Fake {
            fail: Some("check"),
            ..Fake::default()
        };
        assert!(
            cycle(
                &mut browser,
                Path::new("/profile"),
                Duration::ZERO,
                |_| panic!("must not wait")
            )
            .is_err()
        );
        assert_eq!(browser.events, ["check"]);
    }

    #[test]
    fn failed_launch_never_quits_another_browser() {
        let mut browser = Fake {
            fail: Some("open"),
            ..Fake::default()
        };
        assert!(
            cycle(
                &mut browser,
                Path::new("/profile"),
                Duration::ZERO,
                |_| panic!("must not wait")
            )
            .is_err()
        );
        assert_eq!(browser.events, ["check", "open"]);
    }

    #[test]
    fn quit_failure_is_not_reported_as_success() {
        let mut browser = Fake {
            fail: Some("quit"),
            ..Fake::default()
        };
        assert!(cycle(&mut browser, Path::new("/profile"), Duration::ZERO, |_| ()).is_err());
        assert_eq!(browser.events, ["check", "open", "quit"]);
    }
}
