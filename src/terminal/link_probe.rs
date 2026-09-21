//! Whether a path a pane printed actually exists, without blocking the UI.
//!
//! Even a local path can live on an unavailable network drive, so both local
//! and remote panes ask their host off the UI thread. A lookup either finds a
//! recent recorded answer or reports
//! [`Probe::Unknown`](super::search::Probe::Unknown) and remembers the path as
//! wanted; the view then asks the host once for everything wanted and files
//! the replies here, so the *next* hover — a mouse-move away — is a hit.
//!
//! Answers are kept per host, because two hosts can disagree about the same
//! absolute path, and dropped whenever the pane's host changes underneath
//! them.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::ui::host_ops::HostId;

use super::search::Probe;

/// What a host said about one path. A directory is kept apart from a file
/// because a token carrying a line number can only be answered by a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Existence {
    File,
    Dir,
    Missing,
}

impl Existence {
    fn answers(self, require_file: bool) -> Probe {
        match (self, require_file) {
            (Existence::File, _) => Probe::Hit { is_dir: false },
            (Existence::Dir, false) => Probe::Hit { is_dir: true },
            (Existence::Dir, true) | (Existence::Missing, _) => Probe::Miss,
        }
    }
}

/// How many answers one pane keeps. Every hover over a path-shaped word adds
/// at most a handful, and the whole point is to answer the *next* mouse event,
/// so the working set is tiny; the cap only exists so a pane printing
/// thousands of distinct paths cannot grow this without bound.
const MAX_ANSWERS: usize = 512;
const MAX_WANTED: usize = 64;
const MAX_BATCH: usize = 4;
const ANSWER_TTL: Duration = Duration::from_secs(2);
const IN_FLIGHT_TTL: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct LinkProbeCache {
    host: Option<HostId>,
    generation: u64,
    answers: HashMap<PathBuf, (Existence, Instant)>,
    /// Paths a lookup wanted and could not answer. Drained by the view, which
    /// turns them into one host call.
    wanted: VecDeque<PathBuf>,
    /// Paths already out with the host, so a hover repeated every mouse-move
    /// asks once rather than once per frame.
    in_flight: VecDeque<PathBuf>,
    in_flight_since: Option<Instant>,
    retry_after: Option<Instant>,
}

impl LinkProbeCache {
    /// Points the cache at `host`, clearing it if that is not the host the
    /// answers came from. Answers about one machine say nothing about another.
    pub fn retarget(&mut self, host: HostId) {
        if self.host == Some(host) {
            return;
        }
        self.host = Some(host);
        self.generation = self.generation.wrapping_add(1);
        self.answers.clear();
        self.wanted.clear();
        self.in_flight.clear();
        self.in_flight_since = None;
        self.retry_after = None;
    }

    /// Identifies the host assignment and invalidates replies from abandoned batches.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Drops old misses and releases abandoned batches so a stationary hover
    /// can be retried on its next mouse event. Keep positive answers as hints:
    /// opening a link always validates it again off-thread, so forgetting a
    /// known Makefile after two seconds only introduces a click/probe race.
    pub fn expire(&mut self) -> bool {
        let reclaimed = self.reclaim_stalled_batch();
        let now = Instant::now();
        let before = self.answers.len();
        self.answers.retain(|_, (known, checked)| {
            *known != Existence::Missing || now.duration_since(*checked) < ANSWER_TTL
        });
        let retry_due = self.retry_after.is_some_and(|until| now >= until);
        if retry_due {
            self.retry_after = None;
        }
        reclaimed || retry_due || self.answers.len() != before
    }

    /// The cached answer for `path`, recording it as wanted when there is none.
    pub fn probe(&mut self, path: &Path, require_file: bool) -> Probe {
        self.reclaim_stalled_batch();
        let cached = self.answers.get(path).copied();
        if let Some((known, checked)) = cached
            && checked.elapsed() < ANSWER_TTL
        {
            return known.answers(require_file);
        }
        if !self.in_flight.iter().any(|pending| pending == path)
            && !self.wanted.iter().any(|wanted| wanted == path)
            && self.wanted.len() < MAX_WANTED
        {
            self.wanted.push_back(path.to_path_buf());
        }
        // A stale hit remains clickable while it is refreshed. A stale miss
        // cannot rule out a file that has just been created.
        match cached {
            Some((known, _)) if known != Existence::Missing => known.answers(require_file),
            _ => Probe::Unknown,
        }
    }

    fn reclaim_stalled_batch(&mut self) -> bool {
        if !self
            .in_flight_since
            .is_some_and(|started| started.elapsed() >= IN_FLIGHT_TTL)
        {
            return false;
        }
        log::warn!(
            "link probe batch timed out; releasing {} paths",
            self.in_flight.len()
        );
        // HostOps may never invoke its callback if a worker panics. Advancing
        // the generation also keeps a merely slow callback from clearing a
        // newer batch or overwriting its answers when it eventually arrives.
        self.generation = self.generation.wrapping_add(1);
        self.in_flight.clear();
        self.in_flight_since = None;
        true
    }

    /// The paths to ask the host about, moved into the in-flight set so the
    /// next lookup does not ask for them again.
    pub fn take_wanted(&mut self) -> Vec<PathBuf> {
        self.reclaim_stalled_batch();
        if !self.in_flight.is_empty()
            || self.retry_after.is_some_and(|until| Instant::now() < until)
        {
            return Vec::new();
        }
        let count = self.wanted.len().min(MAX_BATCH);
        let wanted: Vec<PathBuf> = self.wanted.drain(..count).collect();
        self.in_flight.extend(wanted.iter().cloned());
        if !wanted.is_empty() {
            self.in_flight_since = Some(Instant::now());
        }
        wanted
    }

    /// Files what the host said. Returns whether anything the cache did not
    /// already know came back — a caller that re-resolves on every answer
    /// would otherwise repaint for replies that change nothing.
    pub fn land(&mut self, generation: u64, answers: Vec<(PathBuf, Existence)>) -> bool {
        if generation != self.generation {
            return false;
        }
        let mut news = false;
        let now = Instant::now();
        for (path, existence) in answers {
            self.in_flight.retain(|pending| pending != &path);
            news |= self
                .answers
                .insert(path, (existence, now))
                .map(|(previous, _)| previous)
                != Some(existence);
        }
        if self.in_flight.is_empty() {
            self.in_flight_since = None;
        }
        // Cheaper than tracking use order, and correct for the same reason the
        // cap is generous: what a hover needs was written microseconds ago, so
        // starting over costs one more round trip at worst.
        if self.answers.len() > MAX_ANSWERS {
            self.answers.clear();
        }
        news
    }

    /// A failed lookup is not evidence that a path is absent. Preserve any
    /// prior hit, release only this batch, and avoid an immediate retry loop.
    pub fn fail(&mut self, generation: u64) {
        if generation != self.generation {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let mut remaining: Vec<_> = self.in_flight.drain(..).collect();
        // The first remaining path may be the syscall still blocked in the
        // retired batch. Give every other path a turn before retrying it.
        if remaining.len() > 1 {
            remaining.rotate_left(1);
        }
        for path in remaining {
            if self.wanted.len() < MAX_WANTED && !self.wanted.contains(&path) {
                self.wanted.push_back(path);
            }
        }
        self.in_flight_since = None;
        self.retry_after = Some(Instant::now() + ANSWER_TTL);
    }

    #[cfg(test)]
    pub fn expire_answers_for_test(&mut self) {
        for (_, checked) in self.answers.values_mut() {
            *checked = Instant::now() - ANSWER_TTL;
        }
    }
}

/// Resolve a click entirely on its captured host. Repository discovery is
/// part of this operation, so a cold hover cache never changes a click's roots.
/// Only NotFound is negative evidence; unavailable roots retain their error.
pub(super) fn resolve_file(
    text: &str,
    click_idx: usize,
    roots: &super::search::LinkRoots,
    host: &dyn crate::ui::host_ops::Host,
    deadline: Instant,
) -> std::io::Result<Option<super::search::LinkMatch>> {
    use std::io::{Error, ErrorKind};
    let resolve = |roots: &super::search::LinkRoots| {
        let mut failure = None;
        let link =
            super::search::link_at(text, click_idx, roots, true, &mut |path, require_file| {
                if failure.is_some() {
                    return Probe::Unknown;
                }
                if Instant::now() >= deadline {
                    failure = Some(Error::from(ErrorKind::TimedOut));
                    return Probe::Unknown;
                }
                match host.stat(path) {
                    Ok(meta) if !require_file || !meta.is_dir => Probe::Hit {
                        is_dir: meta.is_dir,
                    },
                    Ok(_) => Probe::Miss,
                    Err(e)
                        if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) =>
                    {
                        Probe::Miss
                    }
                    Err(e) => {
                        failure = Some(e);
                        Probe::Unknown
                    }
                }
            });
        // An unavailable nearer root must not silently open another root's
        // same-named file. Hover hints may be tentative; explicit opens cannot.
        match failure {
            Some(e) => Err(e),
            None => Ok(link),
        }
    };
    if let Some(link) = resolve(roots)? {
        return Ok(Some(link));
    }
    let relative = super::search::file_candidates_at(text, click_idx)
        .iter()
        .any(|candidate| !candidate.is_rooted(roots.style));
    if relative && roots.dirs.len() == 1 {
        if Instant::now() >= deadline {
            return Err(Error::from(ErrorKind::TimedOut));
        }
        if let Some(cwd) = roots.cwd()
            && let Some(root) = host.repo_root(cwd)?
            && root != cwd
        {
            let mut fallback = roots.clone();
            fallback.dirs = vec![root];
            return resolve(&fallback);
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> HostId {
        HostId::LOCAL
    }

    #[test]
    fn link_lookup_finishes_repository_discovery_and_preserves_timeouts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let cwd = dir.path().join("crates").join("worker");
        std::fs::create_dir_all(&cwd).unwrap();
        let path = dir.path().join("Makefile");
        std::fs::write(&path, b"all:\n").unwrap();
        let host = tty7_core::host::local::LocalHost::new();
        let roots = super::super::search::LinkRoots::local(vec![cwd]);
        let deadline = Instant::now() + Duration::from_secs(5);
        let link = resolve_file("Makefile", 0, &roots, &*host, deadline)
            .unwrap()
            .unwrap();
        assert!(
            matches!(link.target, super::super::search::LinkTarget::File { path: found, .. } if found == path)
        );
        assert!(
            resolve_file("not-present", 0, &roots, &*host, deadline)
                .unwrap()
                .is_none()
        );
        let error = resolve_file("Makefile", 0, &roots, &*host, Instant::now())
            .err()
            .unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn link_probe_failure_preserves_hits_and_releases_only_its_own_batch() {
        let mut cache = LinkProbeCache::default();
        cache.retarget(local());
        let path = PathBuf::from("/repo/Makefile");
        cache.land(cache.generation(), vec![(path.clone(), Existence::File)]);
        cache.expire_answers_for_test();
        assert_eq!(cache.probe(&path, false), Probe::Hit { is_dir: false });
        cache.take_wanted();
        let failed = cache.generation();
        cache.fail(failed);
        assert!(cache.in_flight.is_empty());
        assert_eq!(cache.probe(&path, false), Probe::Hit { is_dir: false });
        assert!(
            cache.take_wanted().is_empty(),
            "errors back off instead of spinning"
        );
        cache.retry_after = Some(Instant::now());
        cache.expire();
        assert_eq!(cache.take_wanted(), vec![path.clone()]);
        cache.fail(failed);
        assert!(
            cache.in_flight.contains(&path),
            "old failures cannot cancel a retry"
        );
        assert!(!cache.land(failed, vec![(path.clone(), Existence::Missing)]));
        cache.land(cache.generation(), vec![(path.clone(), Existence::Missing)]);
        assert_eq!(cache.probe(&path, false), Probe::Miss);
    }

    #[test]
    fn link_probe_retry_rotates_a_blocked_path_behind_unanswered_paths() {
        let mut cache = LinkProbeCache::default();
        cache.retarget(local());
        let paths: Vec<_> = ["slow", "fast", "other", "last"].map(PathBuf::from).into();
        for path in &paths {
            cache.probe(path, false);
        }
        assert_eq!(cache.take_wanted(), paths);
        cache.fail(cache.generation());
        cache.retry_after = Some(Instant::now());
        cache.expire();
        assert_eq!(
            cache.take_wanted(),
            vec![
                paths[1].clone(),
                paths[2].clone(),
                paths[3].clone(),
                paths[0].clone()
            ]
        );
    }

    #[test]
    fn an_unknown_path_is_asked_for_once() {
        let mut cache = LinkProbeCache::default();
        cache.retarget(local());

        assert_eq!(cache.probe(Path::new("/a/b.rs"), true), Probe::Unknown);
        assert_eq!(cache.take_wanted(), vec![PathBuf::from("/a/b.rs")]);

        // Still unknown, but already out with the host — asking again would
        // put one call per frame on the wire for as long as the mouse rests.
        assert_eq!(cache.probe(Path::new("/a/b.rs"), true), Probe::Unknown);
        assert!(cache.take_wanted().is_empty());
    }

    #[test]
    fn a_directory_cannot_answer_a_token_carrying_a_line_number() {
        let mut cache = LinkProbeCache::default();
        cache.retarget(local());
        cache.land(
            cache.generation(),
            vec![(PathBuf::from("/a/logs"), Existence::Dir)],
        );

        assert_eq!(
            cache.probe(Path::new("/a/logs"), false),
            Probe::Hit { is_dir: true }
        );
        assert_eq!(cache.probe(Path::new("/a/logs"), true), Probe::Miss);
    }

    #[test]
    fn a_miss_is_an_answer_and_is_not_asked_for_again() {
        let mut cache = LinkProbeCache::default();
        cache.retarget(local());
        cache.land(
            cache.generation(),
            vec![(PathBuf::from("/a/nope"), Existence::Missing)],
        );

        assert_eq!(cache.probe(Path::new("/a/nope"), false), Probe::Miss);
        assert!(cache.take_wanted().is_empty());
    }

    #[test]
    fn landing_reports_only_answers_that_change_something() {
        let mut cache = LinkProbeCache::default();
        cache.retarget(local());

        assert!(cache.land(
            cache.generation(),
            vec![(PathBuf::from("/a/b.rs"), Existence::File)]
        ));
        assert!(!cache.land(
            cache.generation(),
            vec![(PathBuf::from("/a/b.rs"), Existence::File)]
        ));
        assert!(cache.land(
            cache.generation(),
            vec![(PathBuf::from("/a/b.rs"), Existence::Missing)]
        ));
    }

    #[test]
    fn answers_do_not_survive_a_change_of_host() {
        let mut cache = LinkProbeCache::default();
        cache.retarget(local());
        let local_generation = cache.generation();
        cache.retarget(local());
        assert_eq!(cache.generation(), local_generation);
        cache.land(
            cache.generation(),
            vec![(PathBuf::from("/etc/hosts"), Existence::File)],
        );
        assert_eq!(
            cache.probe(Path::new("/etc/hosts"), false),
            Probe::Hit { is_dir: false }
        );

        cache.retarget(HostId::from_connection_key("ssh-direct:me@box:22"));
        assert_ne!(cache.generation(), local_generation);
        assert_eq!(cache.probe(Path::new("/etc/hosts"), false), Probe::Unknown);
        cache.take_wanted();
        cache.retarget(local());
        assert_ne!(cache.generation(), local_generation);
        assert!(cache.in_flight.is_empty());
        assert!(cache.wanted.is_empty());
    }

    #[test]
    fn expired_hits_remain_clickable_while_hits_and_misses_are_refreshed() {
        let mut cache = LinkProbeCache::default();
        cache.retarget(local());
        let hit = PathBuf::from("/a/Makefile");
        let miss = PathBuf::from("/a/miss");
        cache.land(
            cache.generation(),
            vec![
                (hit.clone(), Existence::File),
                (miss.clone(), Existence::Missing),
            ],
        );
        assert!(!cache.expire());
        cache.expire_answers_for_test();
        assert!(cache.expire(), "only the stale miss is removed");
        assert!(!cache.expire());
        assert_eq!(cache.probe(&hit, true), Probe::Hit { is_dir: false });
        assert_eq!(cache.probe(&miss, false), Probe::Unknown);
        assert_eq!(cache.take_wanted().len(), 2);
        cache.land(cache.generation(), vec![(hit.clone(), Existence::Missing)]);
        assert_eq!(cache.probe(&hit, true), Probe::Miss);
        assert!(cache.take_wanted().is_empty());
    }

    #[test]
    fn abandoned_batches_release_the_gate_and_cannot_overwrite_a_retry() {
        for entry in ["expire", "probe", "take_wanted"] {
            let mut cache = LinkProbeCache::default();
            cache.retarget(local());
            let path = PathBuf::from("/a/Makefile");
            cache.probe(&path, false);
            assert_eq!(cache.take_wanted(), vec![path.clone()]);
            let abandoned = cache.generation();
            cache.in_flight_since = Some(Instant::now() - IN_FLIGHT_TTL);
            match entry {
                "expire" => assert!(cache.expire()),
                "probe" => assert_eq!(cache.probe(&path, false), Probe::Unknown),
                _ => assert!(cache.take_wanted().is_empty()),
            }
            assert_ne!(cache.generation(), abandoned);
            cache.probe(&path, false);
            assert_eq!(cache.take_wanted(), vec![path.clone()]);
            assert!(!cache.land(abandoned, vec![(path.clone(), Existence::Missing)]));
            assert!(
                cache.in_flight.contains(&path),
                "a late reply cannot clear the retry"
            );
            assert!(cache.land(cache.generation(), vec![(path.clone(), Existence::File)]));
            assert_eq!(cache.probe(&path, true), Probe::Hit { is_dir: false });
            assert!(cache.in_flight_since.is_none());
        }
    }

    #[test]
    fn batches_are_bounded_and_only_one_can_be_in_flight() {
        let mut cache = LinkProbeCache::default();
        cache.retarget(local());
        for n in 0..=MAX_WANTED {
            cache.probe(&PathBuf::from(format!("/a/{n}")), false);
        }
        let first = cache.take_wanted();
        assert_eq!(first.len(), MAX_BATCH);
        let next = PathBuf::from("/a/next");
        cache.probe(&next, false);
        cache.probe(&next, false);
        assert!(cache.take_wanted().is_empty());
        assert_eq!(cache.wanted.len(), MAX_WANTED - MAX_BATCH + 1);
        cache.land(
            cache.generation(),
            first
                .into_iter()
                .map(|path| (path, Existence::Missing))
                .collect(),
        );
        assert_eq!(cache.take_wanted().len(), MAX_BATCH);
    }
}
