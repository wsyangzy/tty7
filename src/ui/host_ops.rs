use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use gpui::{App, Context, Window};
use gpui_component::WindowExt as _;

use crate::ui::i18n::{L10nKey, t_fmt};

#[allow(unused_imports)]
pub use tty7_core::host::{
    Entry, Host, HostId, MTime, Meta, Output, SearchHit, SharedHost, WatchSub,
};

mod blocking {
    use std::collections::{HashMap, VecDeque};
    use std::io;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::{Arc, Condvar, Mutex, OnceLock};
    use std::time::Duration;

    use super::HostId;

    type Job = Box<dyn FnOnce() + Send + 'static>;

    const MAX_THREADS: usize = 64;
    const MAX_QUEUED: usize = 256;
    const MAX_LINKS: usize = 16;
    const MAX_LINKS_PER_HOST: usize = 4;
    const MAX_PROBES: usize = 8;
    const MAX_PROBES_PER_HOST: usize = 2;
    const LINGER: Duration = Duration::from_secs(30);

    struct Inner {
        state: Mutex<State>,
        wake: Condvar,
        links: Mutex<HashMap<HostId, LinkCount>>,
    }

    #[derive(Default)]
    struct LinkCount {
        total: usize,
        probes: usize,
    }

    struct State {
        jobs: VecDeque<Job>,
        threads: usize,
        idle: usize,
    }

    impl Inner {
        fn new() -> Self {
            Self {
                state: Mutex::new(State {
                    jobs: VecDeque::new(),
                    threads: 0,
                    idle: 0,
                }),
                wake: Condvar::new(),
                links: Mutex::new(HashMap::new()),
            }
        }
    }

    impl State {
        /// Idle workers already cover `jobs == idle`.
        fn wants_another_thread(&self) -> bool {
            self.jobs.len() > self.idle && self.threads < MAX_THREADS
        }
    }

    /// A submission may arrive between the timeout and reacquiring the lock.
    fn should_retire(timed_out: bool, pending_jobs: usize) -> bool {
        timed_out && pending_jobs == 0
    }

    fn pool() -> &'static Arc<Inner> {
        static POOL: OnceLock<Arc<Inner>> = OnceLock::new();
        POOL.get_or_init(|| Arc::new(Inner::new()))
    }

    // A timed-out UI wait does not stop a filesystem syscall. Keep its slot
    // until the queued closure is discarded or the running closure exits.
    pub(super) struct LinkPermit {
        inner: Arc<Inner>,
        host: HostId,
        background: bool,
    }

    impl LinkPermit {
        fn acquire(inner: &Arc<Inner>, host: HostId, background: bool) -> io::Result<Self> {
            let mut links = inner.links.lock().unwrap_or_else(|e| e.into_inner());
            let host_total = links.get(&host).map_or(0, |count| count.total);
            let host_probes = links.get(&host).map_or(0, |count| count.probes);
            if links.values().map(|count| count.total).sum::<usize>() >= MAX_LINKS
                || host_total >= MAX_LINKS_PER_HOST
                || background
                    && (links.values().map(|count| count.probes).sum::<usize>() >= MAX_PROBES
                        || host_probes >= MAX_PROBES_PER_HOST)
            {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "too many unfinished link operations; retry after they finish",
                ));
            }
            let count = links.entry(host).or_default();
            count.total += 1;
            count.probes += usize::from(background);
            Ok(Self {
                inner: Arc::clone(inner),
                host,
                background,
            })
        }
    }

    impl Drop for LinkPermit {
        fn drop(&mut self) {
            let mut links = self.inner.links.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(count) = links.get_mut(&self.host) {
                count.total -= 1;
                count.probes -= usize::from(self.background);
                if count.total == 0 {
                    links.remove(&self.host);
                }
            }
        }
    }

    pub(super) fn link_permit(host: HostId, background: bool) -> io::Result<LinkPermit> {
        LinkPermit::acquire(pool(), host, background)
    }

    pub(super) fn submit(job: impl FnOnce() + Send + 'static, bounded: bool) -> io::Result<()> {
        submit_on(pool(), Box::new(job), bounded, |inner| {
            std::thread::Builder::new()
                .name("tty7-host-op".into())
                .spawn(move || worker(inner, LINGER))
                .map(|_| ())
        })
    }

    fn submit_on(
        inner: &Arc<Inner>,
        job: Job,
        bounded: bool,
        spawn: impl FnOnce(Arc<Inner>) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut st = inner.state.lock().unwrap_or_else(|e| e.into_inner());
        // Legacy callers may carry a save or a pending-state release. Preserve
        // their admission contract; only result-aware link work may be rejected.
        if bounded && st.jobs.len() >= MAX_QUEUED {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "host operation queue is full",
            ));
        }
        st.jobs.push_back(job);
        if st.wants_another_thread() {
            st.threads += 1;
            if let Err(e) = spawn(Arc::clone(inner)) {
                st.threads -= 1;
                log::warn!("could not start a host-op thread: {e}");
                if st.threads == 0 {
                    st.jobs.pop_back();
                    return Err(e);
                }
            }
        }
        drop(st);
        inner.wake.notify_one();
        Ok(())
    }

    fn worker(inner: Arc<Inner>, linger: Duration) {
        struct WorkerExit {
            inner: Arc<Inner>,
            retired: bool,
        }
        impl Drop for WorkerExit {
            fn drop(&mut self) {
                if !self.retired {
                    self.inner
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .threads -= 1;
                }
            }
        }
        let mut exit = WorkerExit {
            inner: Arc::clone(&inner),
            retired: false,
        };
        loop {
            let job = {
                let mut st = inner.state.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if let Some(job) = st.jobs.pop_front() {
                        break job;
                    }
                    st.idle += 1;
                    let (guard, timeout) = inner
                        .wake
                        .wait_timeout(st, linger)
                        .unwrap_or_else(|e| e.into_inner());
                    st = guard;
                    st.idle -= 1;
                    if should_retire(timeout.timed_out(), st.jobs.len()) {
                        // Retire under the same lock used by submit; otherwise
                        // it could count this worker and leave its new job idle.
                        st.threads -= 1;
                        exit.retired = true;
                        return;
                    }
                }
            };
            if catch_unwind(AssertUnwindSafe(job)).is_err() {
                log::error!("host-op job panicked; worker remains available");
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn link_worker_survives_a_panicking_job_and_retires_cleanly() {
            let inner = Arc::new(Inner::new());
            let (tx, rx) = std::sync::mpsc::channel();
            {
                let mut st = inner.state.lock().unwrap();
                st.threads = 1;
                st.jobs.push_back(Box::new(|| panic!("injected job panic")));
                st.jobs.push_back(Box::new(move || tx.send(()).unwrap()));
            }
            let work = Arc::clone(&inner);
            let thread = std::thread::spawn(move || worker(work, Duration::from_millis(1)));
            let result = rx.recv_timeout(Duration::from_secs(2));
            thread.join().unwrap();
            result.unwrap();
            let st = inner.state.lock().unwrap();
            assert_eq!((st.threads, st.idle), (0, 0));
            assert!(st.jobs.is_empty());
        }

        #[test]
        fn link_submission_rejection_never_executes_inline() {
            let inner = Arc::new(Inner::new());
            let result = submit_on(
                &inner,
                Box::new(|| panic!("rejected work must not execute")),
                true,
                |_| Err(io::Error::other("injected thread creation failure")),
            );
            assert!(result.is_err());
            {
                let mut st = inner.state.lock().unwrap();
                assert_eq!(st.threads, 0);
                assert!(st.jobs.is_empty());
                *st = state(MAX_QUEUED, MAX_THREADS, 0);
            }
            let result = submit_on(
                &inner,
                Box::new(|| panic!("full queue must reject work")),
                true,
                |_| panic!("a full queue must not spawn"),
            );
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WouldBlock);
            assert_eq!(inner.state.lock().unwrap().jobs.len(), MAX_QUEUED);
            submit_on(&inner, Box::new(|| {}), false, |_| {
                panic!("existing workers are at their ceiling")
            })
            .unwrap();
            assert_eq!(
                inner.state.lock().unwrap().jobs.len(),
                MAX_QUEUED + 1,
                "legacy work must retain its queue admission contract"
            );
        }

        #[test]
        fn link_capacity_is_bounded_per_host_and_globally_and_released_on_drop() {
            let inner = Arc::new(Inner::new());
            let mut permits = Vec::new();
            for _ in 0..MAX_LINKS_PER_HOST {
                permits.push(LinkPermit::acquire(&inner, HostId::LOCAL, false).unwrap());
            }
            assert!(LinkPermit::acquire(&inner, HostId::LOCAL, false).is_err());
            permits.pop();
            permits.push(LinkPermit::acquire(&inner, HostId::LOCAL, false).unwrap());
            for n in MAX_LINKS_PER_HOST..MAX_LINKS {
                permits.push(LinkPermit::acquire(&inner, HostId(n as u64), false).unwrap());
            }
            assert!(LinkPermit::acquire(&inner, HostId(u64::MAX), false).is_err());
            drop(permits);
            assert!(inner.links.lock().unwrap().is_empty());
        }

        #[test]
        fn link_probes_leave_room_for_clicks_on_the_same_host_and_other_hosts() {
            let inner = Arc::new(Inner::new());
            let mut probes = Vec::new();
            for n in 0..MAX_PROBES {
                probes.push(LinkPermit::acquire(&inner, HostId((n / 2) as u64), true).unwrap());
            }
            assert!(LinkPermit::acquire(&inner, HostId::LOCAL, true).is_err());
            assert!(LinkPermit::acquire(&inner, HostId(u64::MAX), true).is_err());
            let click = LinkPermit::acquire(&inner, HostId::LOCAL, false).unwrap();
            let second_click = LinkPermit::acquire(&inner, HostId::LOCAL, false).unwrap();
            assert!(LinkPermit::acquire(&inner, HostId::LOCAL, false).is_err());
            drop((click, second_click, probes));
            assert!(inner.links.lock().unwrap().is_empty());
        }

        fn state(jobs: usize, threads: usize, idle: usize) -> State {
            let mut q: VecDeque<Job> = VecDeque::new();
            for _ in 0..jobs {
                q.push_back(Box::new(|| {}));
            }
            State {
                jobs: q,
                threads,
                idle,
            }
        }

        #[test]
        fn an_idle_thread_is_preferred_over_a_new_one() {
            assert!(
                !state(1, 1, 1).wants_another_thread(),
                "one job and one idle thread needs nobody new"
            );
            assert!(
                !state(2, 2, 2).wants_another_thread(),
                "jobs == idle is already covered"
            );
            assert!(
                state(3, 2, 2).wants_another_thread(),
                "one job more than there are idle threads"
            );
        }

        #[test]
        fn the_first_job_starts_the_first_thread() {
            assert!(state(1, 0, 0).wants_another_thread());
        }

        #[test]
        fn the_pool_stops_growing_at_its_ceiling() {
            assert!(state(1000, MAX_THREADS - 1, 0).wants_another_thread());
            assert!(
                !state(1000, MAX_THREADS, 0).wants_another_thread(),
                "a backlog does not buy more than MAX_THREADS"
            );
        }

        #[test]
        fn a_worker_retires_only_on_a_timeout_with_nothing_queued() {
            assert!(should_retire(true, 0));
            assert!(!should_retire(false, 0), "a wake-up is not a timeout");
        }

        /// The whole reason the queue is consulted: `submit` counted this
        /// thread as idle and therefore did not spawn one, so retiring here
        /// would leave the job it just pushed with no worker.
        #[test]
        fn a_job_that_landed_during_the_timeout_keeps_the_worker_alive() {
            assert!(!should_retire(true, 1));
        }
    }
}

async fn off_thread<T, F>(f: F) -> std::io::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    off_thread_with_admission(f, false).await
}

async fn off_thread_with_admission<T, F>(f: F, bounded: bool) -> std::io::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = smol::channel::bounded(1);
    blocking::submit(
        move || {
            if tx.is_closed() {
                return;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
                .map_err(|_| std::io::Error::other("host operation panicked"));
            let _ = tx.send_blocking(result);
        },
        bounded,
    )?;
    rx.recv()
        .await
        .map_err(|_| std::io::Error::other("host operation ended without a result"))?
}

pub const LINK_OP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

async fn link_task<T>(
    host: HostId,
    background: bool,
    f: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> std::io::Result<T>
where
    T: Send + 'static,
{
    link_task_with_timeout(host, background, LINK_OP_TIMEOUT, f).await
}

async fn link_task_with_timeout<T>(
    host: HostId,
    background: bool,
    timeout: std::time::Duration,
    f: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> std::io::Result<T>
where
    T: Send + 'static,
{
    let deadline = std::time::Instant::now() + timeout;
    let permit = blocking::link_permit(host, background)?;
    smol::future::or(
        async move {
            off_thread_with_admission(
                move || {
                    let _permit = permit;
                    if std::time::Instant::now() >= deadline {
                        return Err(link_timeout());
                    }
                    let result = f();
                    // The result and timer can both be ready before the UI
                    // polls again. Never publish a result completed too late.
                    // This cannot cancel OS work or undo its external effects.
                    if std::time::Instant::now() >= deadline {
                        Err(link_timeout())
                    } else {
                        result
                    }
                },
                true,
            )
            .await?
        },
        async move {
            smol::Timer::at(deadline).await;
            Err(link_timeout())
        },
    )
    .await
}

fn link_timeout() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::TimedOut, "link operation timed out")
}

#[cfg(test)]
mod link_task_tests {
    use super::*;
    use std::io;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::{Duration, Instant};

    #[test]
    fn link_panics_are_results_and_the_next_operation_can_succeed() {
        let result: io::Result<()> = smol::block_on(off_thread(|| panic!("injected host panic")));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(smol::block_on(off_thread(|| 7)).unwrap(), 7);
    }

    #[test]
    fn link_work_past_its_deadline_does_not_execute() {
        let ran = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&ran);
        let result = smol::block_on(link_task_with_timeout(
            HostId::from_connection_key("test:expired-link"),
            false,
            Duration::ZERO,
            move || {
                seen.store(true, Ordering::SeqCst);
                Ok(())
            },
        ));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[test]
    fn link_timeout_keeps_capacity_until_the_real_worker_finishes() {
        let host = HostId::from_connection_key("test:blocked-link");
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let result = smol::block_on(link_task_with_timeout(
            host,
            false,
            Duration::from_secs(1),
            move || {
                started_tx.send(()).unwrap();
                let _ = release_rx.recv();
                Ok(())
            },
        ));
        // Always release the worker before an assertion could unwind this test.
        let started = started_rx.try_recv().is_ok();
        let permits: Vec<_> = (0..3)
            .map(|_| blocking::link_permit(host, false).unwrap())
            .collect();
        let blocked = blocking::link_permit(host, false).is_err();
        let _ = release_tx.send(());
        assert!(
            started,
            "worker must have started before exercising running-IO timeout"
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(
            blocked,
            "timing out must not make running IO's slot available"
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(permit) = blocking::link_permit(host, false) {
                drop(permit);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "completed worker did not release its slot"
            );
            std::thread::yield_now();
        }
        drop(permits);
    }
}

pub struct HostOps;

impl HostOps {
    pub fn run<T, E, F, L>(host: SharedHost, cx: &mut Context<E>, f: F, land: L)
    where
        E: 'static,
        T: Send + 'static,
        F: FnOnce(&dyn Host) -> T + Send + 'static,
        L: FnOnce(&mut E, T, &mut Context<E>) + 'static,
    {
        tty7_core::host::register_ui_thread();
        cx.spawn(async move |this, cx| {
            let out = match off_thread(move || f(&*host)).await {
                Ok(out) => out,
                Err(e) => {
                    log::error!("host operation failed: {e}");
                    return;
                }
            };
            let _ = this.update(cx, |view, cx| land(view, out, cx));
        })
        .detach();
    }

    pub fn run_detached<T, E, F, L>(host: SharedHost, cx: &mut Context<E>, f: F, land: L)
    where
        E: 'static,
        T: Send + 'static,
        F: FnOnce(&dyn Host) -> T + Send + 'static,
        L: FnOnce(&mut App, T) + 'static,
    {
        tty7_core::host::register_ui_thread();
        cx.spawn(async move |_this, cx| {
            let out = match off_thread(move || f(&*host)).await {
                Ok(out) => out,
                Err(e) => {
                    log::error!("host operation failed: {e}");
                    return;
                }
            };
            cx.update(|cx| land(cx, out));
        })
        .detach();
    }

    pub fn run_in<T, E, F, L>(host: SharedHost, window: &Window, cx: &mut Context<E>, f: F, land: L)
    where
        E: 'static,
        T: Send + 'static,
        F: FnOnce(&dyn Host) -> T + Send + 'static,
        L: FnOnce(&mut E, T, &mut Window, &mut Context<E>) + 'static,
    {
        tty7_core::host::register_ui_thread();
        cx.spawn_in(window, async move |this, cx| {
            let out = match off_thread(move || f(&*host)).await {
                Ok(out) => out,
                Err(e) => {
                    log::error!("host operation failed: {e}");
                    return;
                }
            };
            let _ = this.update_in(cx, |view, window, cx| land(view, out, window, cx));
        })
        .detach();
    }

    /// Link reads always land a result, including rejection, panic and timeout.
    /// The worker retains its capacity until the underlying IO actually ends.
    pub fn run_link<T, E, F, L>(host: SharedHost, cx: &mut Context<E>, f: F, land: L)
    where
        E: 'static,
        T: Send + 'static,
        F: FnOnce(&dyn Host) -> std::io::Result<T> + Send + 'static,
        L: FnOnce(&mut E, std::io::Result<T>, &mut Context<E>) + 'static,
    {
        tty7_core::host::register_ui_thread();
        cx.spawn(async move |this, cx| {
            let out = link_task(host.id(), true, move || f(&*host)).await;
            let _ = this.update(cx, |view, cx| land(view, out, cx));
        })
        .detach();
    }

    pub fn run_link_in<T, E, F, L>(
        host: SharedHost,
        window: &Window,
        cx: &mut Context<E>,
        f: F,
        land: L,
    ) where
        E: 'static,
        T: Send + 'static,
        F: FnOnce(&dyn Host) -> std::io::Result<T> + Send + 'static,
        L: FnOnce(&mut E, std::io::Result<T>, &mut Window, &mut Context<E>) + 'static,
    {
        Self::run_link_task_in(host.id(), window, cx, move || f(&*host), land);
    }

    pub fn run_link_task_in<T, E, F, L>(
        host: HostId,
        window: &Window,
        cx: &mut Context<E>,
        f: F,
        land: L,
    ) where
        E: 'static,
        T: Send + 'static,
        F: FnOnce() -> std::io::Result<T> + Send + 'static,
        L: FnOnce(&mut E, std::io::Result<T>, &mut Window, &mut Context<E>) + 'static,
    {
        tty7_core::host::register_ui_thread();
        cx.spawn_in(window, async move |this, cx| {
            let out = link_task(host, false, f).await;
            let _ = this.update_in(cx, |view, window, cx| land(view, out, window, cx));
        })
        .detach();
    }

    pub fn run_or_notify<T, E, F, L>(
        host: SharedHost,
        window: &Window,
        cx: &mut Context<E>,
        context: impl Into<String>,
        f: F,
        land: L,
    ) where
        E: 'static,
        T: Send + 'static,
        F: FnOnce(&dyn Host) -> std::io::Result<T> + Send + 'static,
        L: FnOnce(&mut E, T, &mut Window, &mut Context<E>) + 'static,
    {
        let context = context.into();
        Self::run_in(
            host,
            window,
            cx,
            f,
            move |view, result, window, cx| match result {
                Ok(value) => land(view, value, window, cx),
                Err(e) => HostOps::notify_err(window, cx, &context, &e),
            },
        );
    }

    pub fn notify_err(window: &mut Window, cx: &mut App, context: &str, err: &std::io::Error) {
        window.push_notification(
            t_fmt(
                L10nKey::HostOpsError,
                &[("context", context), ("error", &explain_io(err))],
            ),
            cx,
        );
    }
}

/// A sentence for the failures someone can act on, and the raw error for the
/// rest. `Display` on an `io::Error` answers "what happened" for a developer
/// reading a log; it does not answer "what now" for the person who just lost
/// a save, and "Permission denied (os error 13)" is the shape of that gap.
pub fn explain_io(err: &std::io::Error) -> String {
    use std::io::ErrorKind;
    let key = match err.kind() {
        ErrorKind::PermissionDenied => L10nKey::IoDenied,
        ErrorKind::NotFound => L10nKey::IoGone,
        ErrorKind::StorageFull => L10nKey::IoNoSpace,
        ErrorKind::ReadOnlyFilesystem => L10nKey::IoReadOnly,
        ErrorKind::ResourceBusy => L10nKey::IoBusy,
        ErrorKind::TimedOut => L10nKey::IoTimedOut,
        _ => return err.to_string(),
    };
    crate::ui::i18n::t(key).to_string()
}

pub struct InFlight<K: Eq + Hash + Clone> {
    in_flight: HashSet<K>,
    stale: HashSet<K>,
}

impl<K: Eq + Hash + Clone> Default for InFlight<K> {
    fn default() -> Self {
        InFlight {
            in_flight: HashSet::new(),
            stale: HashSet::new(),
        }
    }
}

impl<K: Eq + Hash + Clone> InFlight<K> {
    pub fn begin(&mut self, key: K) -> bool {
        self.in_flight.insert(key)
    }

    pub fn invalidate(&mut self, key: &K) {
        if self.in_flight.contains(key) {
            self.stale.insert(key.clone());
        }
    }

    pub fn invalidate_all(&mut self) {
        self.stale.extend(self.in_flight.iter().cloned());
    }

    /// Drop every key the predicate rejects — bookkeeping for work that will
    /// never land (a host cleared away under an in-flight job). If the job
    /// does land after all, its `finish` is a no-op rather than a poison.
    pub fn retain(&mut self, keep: impl Fn(&K) -> bool) {
        self.in_flight.retain(|k| keep(k));
        self.stale.retain(|k| keep(k));
    }

    pub fn finish(&mut self, key: &K) -> bool {
        self.in_flight.remove(key);
        !self.stale.remove(key)
    }

    pub fn is_pending(&self, key: &K) -> bool {
        self.in_flight.contains(key)
    }

    pub fn pending_keys(&self) -> impl Iterator<Item = &K> {
        self.in_flight.iter()
    }

    pub fn len(&self) -> usize {
        self.in_flight.len()
    }

    pub fn is_empty(&self) -> bool {
        self.in_flight.is_empty()
    }
}

pub struct ByHost<K: Eq + Hash, V> {
    map: HashMap<HostId, HashMap<K, V>>,
}

impl<K: Eq + Hash, V> Default for ByHost<K, V> {
    fn default() -> Self {
        ByHost {
            map: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash, V> ByHost<K, V> {
    pub fn get<Q>(&self, host: HostId, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.map.get(&host)?.get(key)
    }

    pub fn insert(&mut self, host: HostId, key: K, value: V) -> Option<V> {
        self.map.entry(host).or_default().insert(key, value)
    }

    pub fn remove<Q>(&mut self, host: HostId, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.map.get_mut(&host)?.remove(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = (HostId, &K)> {
        self.map
            .iter()
            .flat_map(|(host, inner)| inner.keys().map(move |k| (*host, k)))
    }

    pub fn clear_host(&mut self, host: HostId) {
        self.map.remove(&host);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn len(&self) -> usize {
        self.map.values().map(HashMap::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.map.values().all(HashMap::is_empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_failures_people_can_act_on_get_a_sentence() {
        use std::io::{Error, ErrorKind};
        crate::ui::i18n::set_locale("en");

        // The kinds that change what you would do next.
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::StorageFull,
            ErrorKind::ReadOnlyFilesystem,
            ErrorKind::NotFound,
        ] {
            let text = explain_io(&Error::new(kind, "os error 13"));
            assert!(!text.contains("os error"), "{kind:?}: {text}");
            assert!(text.ends_with('.'), "{kind:?}: {text}");
        }

        // Anything unclassified keeps its detail rather than losing it to a
        // vague house sentence.
        assert_eq!(
            explain_io(&Error::other("the widget frobnicated")),
            "the widget frobnicated"
        );
    }
    use std::path::{Path, PathBuf};

    #[test]
    fn in_flight_tracks_supersession() {
        let mut loads: InFlight<PathBuf> = InFlight::default();
        let a = PathBuf::from("/a");
        let b = PathBuf::from("/b");

        assert!(loads.begin(a.clone()), "first request spawns");
        assert!(!loads.begin(a.clone()), "a repeat frame does not");
        assert!(loads.is_pending(&a));
        assert_eq!(loads.len(), 1);

        assert!(loads.finish(&a));
        assert!(!loads.is_pending(&a));
        assert!(loads.is_empty());

        assert!(loads.begin(a.clone()));
        loads.invalidate(&a);
        assert!(!loads.finish(&a));
        assert!(loads.begin(a.clone()));
        assert!(loads.finish(&a));

        loads.invalidate(&b);
        assert!(loads.begin(b.clone()));
        assert!(loads.finish(&b));
    }

    #[test]
    fn invalidate_all_covers_everything_in_flight() {
        let mut loads: InFlight<u32> = InFlight::default();
        loads.begin(1);
        loads.begin(2);
        loads.invalidate_all();
        assert!(!loads.finish(&1));
        assert!(!loads.finish(&2));
        assert!(loads.is_empty());

        loads.begin(3);
        assert!(loads.finish(&3));
    }

    #[test]
    fn by_host_keys_by_machine_as_well_as_path() {
        let remote = HostId::from_connection_key("ssh-direct:me@box:22");
        let mut cache: ByHost<PathBuf, &str> = ByHost::default();
        let p = PathBuf::from("/home/me/proj");

        cache.insert(HostId::LOCAL, p.clone(), "local listing");
        cache.insert(remote, p.clone(), "remote listing");
        assert_eq!(cache.get(HostId::LOCAL, &p), Some(&"local listing"));
        assert_eq!(cache.get(remote, &p), Some(&"remote listing"));
        assert_eq!(cache.len(), 2);

        cache.clear_host(remote);
        assert_eq!(cache.get(remote, &p), None);
        assert_eq!(cache.get(HostId::LOCAL, &p), Some(&"local listing"));

        cache.remove(HostId::LOCAL, &p);
        assert!(cache.is_empty());
    }

    #[test]
    fn lookups_borrow_the_key_rather_than_cloning_it() {
        let mut cache: ByHost<PathBuf, u32> = ByHost::default();
        cache.insert(HostId::LOCAL, PathBuf::from("/a/b"), 1);
        assert_eq!(cache.get(HostId::LOCAL, Path::new("/a/b")), Some(&1));
        assert_eq!(cache.remove(HostId::LOCAL, Path::new("/a/b")), Some(1));
        assert!(cache.is_empty());
    }
}

#[cfg(test)]
mod gpui_tests {
    use crate::ui::host_ops::{Host, HostOps};
    use gpui::{App, AppContext, Context, Entity, TestAppContext};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Pane;

    #[gpui::test]
    fn a_detached_result_lands_after_its_view_is_dropped(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let detached: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let view_scoped: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

        let pane: Entity<Pane> = cx.new(|_cx: &mut Context<Pane>| Pane);
        let _: () = pane.update(cx, |_pane: &mut Pane, cx: &mut Context<Pane>| {
            let d = Arc::clone(&detached);
            HostOps::run_detached(
                tty7_core::host::local::LocalHost::new(),
                cx,
                |_h: &dyn Host| 7usize,
                move |_app: &mut App, n: usize| {
                    d.fetch_add(n, Ordering::SeqCst);
                },
            );
            let v = Arc::clone(&view_scoped);
            HostOps::run(
                tty7_core::host::local::LocalHost::new(),
                cx,
                |_h: &dyn Host| 7usize,
                move |_pane: &mut Pane, n: usize, _cx: &mut Context<Pane>| {
                    v.fetch_add(n, Ordering::SeqCst);
                },
            );
        });

        drop(pane);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while detached.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            cx.background_executor.run_until_parked();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        for _ in 0..20 {
            cx.background_executor.run_until_parked();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        assert_eq!(
            detached.load(Ordering::SeqCst),
            7,
            "run_detached must land: it is what releases the shared claim"
        );
        assert_eq!(
            view_scoped.load(Ordering::SeqCst),
            0,
            "run is view-scoped and must not run against a dead view"
        );
    }
}
