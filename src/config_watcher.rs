//! Watches the config file and requests a reload once edits settle.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// How long the config file must stay unchanged before it is reloaded.
///
/// Editors often save in several steps (truncate, then write; or write a
/// temporary file, then rename). Reloading only after the burst ends avoids
/// loading a half-written file.
const RELOAD_DEBOUNCE: Duration = Duration::from_millis(500);

/// Watch `config_path` and call `on_reload` once each burst of changes has
/// settled. Runs until `on_reload` returns `false` or the watcher fails.
pub fn watch_config_file(config_path: PathBuf, mut on_reload: impl FnMut() -> bool) -> Result<()> {
    let (watcher_tx, watcher_rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = watcher_tx.send(res);
        },
        notify::Config::default(),
    )?;

    // Watch the parent directory so atomic replaces (rename) are caught.
    if let Some(parent) = config_path.parent() {
        watcher.watch(parent, RecursiveMode::NonRecursive)?;
        tracing::info!("Watching config directory: {}", parent.display());
    }

    let mut debouncer = Debouncer::new(RELOAD_DEBOUNCE);

    loop {
        let received = match debouncer.time_until_due(Instant::now()) {
            Some(timeout) => watcher_rx.recv_timeout(timeout),
            None => watcher_rx.recv().map_err(RecvTimeoutError::from),
        };

        match received {
            Ok(Ok(event)) if is_config_change(&event, &config_path) => {
                tracing::debug!("Config file changed, reloading once changes settle");
                debouncer.trigger(Instant::now());
            }
            Ok(Ok(_)) | Err(RecvTimeoutError::Timeout) => {}
            Ok(Err(e)) => tracing::warn!("File watcher error: {}", e),
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if debouncer.poll(Instant::now()) && !on_reload() {
            break;
        }
    }

    Ok(())
}

/// Whether `event` creates or modifies the config file itself.
fn is_config_change(event: &Event, config_path: &Path) -> bool {
    matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_))
        && event.paths.iter().any(|p| p == config_path)
}

/// Trailing-edge debounce: due once `delay` has passed since the last
/// trigger. The current time is passed in so the logic is testable.
#[derive(Debug)]
struct Debouncer {
    delay: Duration,
    last_trigger: Option<Instant>,
}

impl Debouncer {
    fn new(delay: Duration) -> Self {
        Self {
            delay,
            last_trigger: None,
        }
    }

    fn trigger(&mut self, now: Instant) {
        self.last_trigger = Some(now);
    }

    /// Time left until the pending trigger is due, or `None` if none is pending.
    fn time_until_due(&self, now: Instant) -> Option<Duration> {
        self.last_trigger
            .map(|last| (last + self.delay).saturating_duration_since(now))
    }

    /// Whether the pending trigger is due. Clears it if so, so it fires once.
    fn poll(&mut self, now: Instant) -> bool {
        let due = self.time_until_due(now) == Some(Duration::ZERO);
        if due {
            self.last_trigger = None;
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};

    const DELAY: Duration = Duration::from_millis(500);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn test_debouncer_idle_is_never_due() {
        let start = Instant::now();
        let mut debouncer = Debouncer::new(DELAY);

        assert_eq!(debouncer.time_until_due(start), None);
        assert!(!debouncer.poll(start + ms(10_000)));
    }

    #[test]
    fn test_debouncer_fires_once_after_delay() {
        let start = Instant::now();
        let mut debouncer = Debouncer::new(DELAY);
        debouncer.trigger(start);

        assert_eq!(debouncer.time_until_due(start + ms(100)), Some(ms(400)));
        assert!(!debouncer.poll(start + ms(499)));
        assert!(debouncer.poll(start + ms(500)));
        // Cleared once fired.
        assert!(!debouncer.poll(start + ms(10_000)));
        assert_eq!(debouncer.time_until_due(start + ms(10_000)), None);
    }

    #[test]
    fn test_debouncer_overdue_is_due_immediately() {
        let start = Instant::now();
        let mut debouncer = Debouncer::new(DELAY);
        debouncer.trigger(start);

        assert_eq!(
            debouncer.time_until_due(start + ms(900)),
            Some(Duration::ZERO)
        );
        assert!(debouncer.poll(start + ms(900)));
    }

    #[test]
    fn test_debouncer_burst_fires_once_after_last_event() {
        let start = Instant::now();
        let mut debouncer = Debouncer::new(DELAY);
        let mut fired = 0;

        // 10 events, 100ms apart: the deadline keeps moving.
        for i in 0..10 {
            let now = start + ms(i * 100);
            debouncer.trigger(now);
            if debouncer.poll(now) {
                fired += 1;
            }
        }
        assert_eq!(fired, 0);

        let last = start + ms(900);
        assert!(!debouncer.poll(last + ms(499)));
        assert!(debouncer.poll(last + ms(500)));
    }

    #[test]
    fn test_debouncer_truncate_then_write_reloads_after_write() {
        // An editor truncating the file, then writing it 5ms later, must not
        // reload the empty file.
        let start = Instant::now();
        let mut debouncer = Debouncer::new(DELAY);

        debouncer.trigger(start); // truncate
        debouncer.trigger(start + ms(5)); // write

        assert!(!debouncer.poll(start + ms(500)));
        assert!(debouncer.poll(start + ms(505)));
    }

    fn event(kind: EventKind, path: &str) -> Event {
        Event::new(kind).add_path(PathBuf::from(path))
    }

    #[test]
    fn test_is_config_change() {
        let config = Path::new("/home/u/.config/nirimap/config.toml");

        assert!(is_config_change(
            &event(
                EventKind::Modify(ModifyKind::Any),
                "/home/u/.config/nirimap/config.toml"
            ),
            config
        ));
        assert!(is_config_change(
            &event(
                EventKind::Create(CreateKind::File),
                "/home/u/.config/nirimap/config.toml"
            ),
            config
        ));
        assert!(!is_config_change(
            &event(
                EventKind::Remove(RemoveKind::File),
                "/home/u/.config/nirimap/config.toml"
            ),
            config
        ));
        assert!(!is_config_change(
            &event(
                EventKind::Access(AccessKind::Any),
                "/home/u/.config/nirimap/config.toml"
            ),
            config
        ));
        assert!(!is_config_change(
            &event(
                EventKind::Modify(ModifyKind::Any),
                "/home/u/.config/nirimap/other.toml"
            ),
            config
        ));
    }
}
