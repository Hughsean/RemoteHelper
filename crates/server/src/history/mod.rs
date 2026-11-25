//! 服务端历史记录器。只有记录器线程可以修改历史存储。

pub(crate) mod clock;
#[cfg(test)]
mod e2e_tests;
mod retention;
mod store;

use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, RwLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ::history::{Frame, GapReason, Id, Session};
use retention::Retention;
pub(crate) use retention::{ClockAckError, DayEvidence, RetentionSnapshot};
use store::{Activity, ReadState, Store};
pub(crate) use store::{HistorySnapshot, Page, QueryError, StorageState, TailCursor, atomic_write};

const QUEUE_LIMIT: usize = 256;
const RETRY_INTERVAL: Duration = Duration::from_secs(5);
const SYNC_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub(crate) struct Sample {
    pub session: Session,
    pub frame: Frame,
}

#[derive(Clone, Debug)]
pub(crate) struct RecorderOptions {
    pub enabled: bool,
    pub seal_interval: Duration,
    pub retention_days: u32,
    pub retention_bytes: u64,
    pub compression_level: i32,
}

#[derive(Clone)]
pub(crate) struct RecorderHandle {
    inbox: Arc<Inbox>,
    read: Arc<RwLock<ReadState>>,
}

pub(crate) struct Recorder {
    handle: RecorderHandle,
    done: mpsc::Receiver<io::Result<()>>,
    thread: Option<JoinHandle<()>>,
}

struct Inbox {
    pending: Mutex<Pending>,
    ready: Condvar,
}

#[derive(Default)]
struct Pending {
    queue: VecDeque<Message>,
    samples: usize,
    dropped: Option<Loss>,
    closed: bool,
}

enum Message {
    Sample(Sample),
    Overrun(Loss),
    Configure(RecorderOptions),
    AckClock {
        store_id: Id,
        event_id: Id,
        displayed_wall_ms: i64,
        issued_at: Instant,
        reply: tokio::sync::oneshot::Sender<Result<(), ClockAckError>>,
    },
    Close,
}

#[derive(Clone)]
struct Loss {
    first: Sample,
    last: Sample,
}

impl Loss {
    fn push(range: &mut Option<Self>, sample: &Sample) {
        match range {
            Some(loss) => loss.last = sample.clone(),
            None => {
                *range = Some(Self {
                    first: sample.clone(),
                    last: sample.clone(),
                })
            }
        }
    }

    fn start_after(&self, previous: Option<&Activity>) -> Option<i64> {
        let previous = previous.filter(|p| {
            p.sample.session.session_id == self.first.session.session_id
                && p.sample.frame.sample_seq >= self.first.frame.sample_seq
        });
        if let Some(p) = previous {
            if p.sample.frame.sample_seq >= self.last.frame.sample_seq {
                return None;
            }
            return p
                .sample
                .frame
                .clock
                .wall_ms
                .map(|t| t.saturating_add(i64::from(p.sample.frame.period_ms)));
        }
        self.first
            .frame
            .clock
            .wall_ms
            .or(self.last.frame.clock.wall_ms)
    }
}

impl RecorderHandle {
    /// 不等待记录器线程和磁盘。队列溢出时在下一条已接收样本或控制栅栏前插入丢样范围。
    pub fn record(&self, sample: Sample) -> bool {
        let mut pending = self.inbox.pending.lock().unwrap();
        if pending.closed {
            return false;
        }
        if pending.samples == QUEUE_LIMIT {
            Loss::push(&mut pending.dropped, &sample);
            return false;
        }
        pending.flush_dropped();
        pending.samples += 1;
        pending.queue.push_back(Message::Sample(sample));
        self.inbox.ready.notify_one();
        true
    }

    /// 入队锁保证控制指令和采样顺序一致；仅样本占用 FIFO 配额。
    pub fn configure(&self, options: RecorderOptions) -> bool {
        let mut pending = self.inbox.pending.lock().unwrap();
        if pending.closed {
            return false;
        }
        pending.flush_dropped();
        pending.queue.push_back(Message::Configure(options));
        self.inbox.ready.notify_one();
        true
    }

    pub async fn ack_clock(
        &self,
        store_id: Id,
        event_id: Id,
        displayed_wall_ms: i64,
        issued_at: Instant,
    ) -> Result<(), ClockAckError> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        {
            let mut pending = self.inbox.pending.lock().unwrap();
            if pending.closed {
                return Err(ClockAckError::Storage);
            }
            pending.flush_dropped();
            pending.queue.push_back(Message::AckClock {
                store_id,
                event_id,
                displayed_wall_ms,
                issued_at,
                reply,
            });
            self.inbox.ready.notify_one();
        }
        receive.await.unwrap_or(Err(ClockAckError::Storage))
    }

    pub fn snapshot(&self) -> Arc<HistorySnapshot> {
        self.read.read().unwrap().snapshot()
    }

    /// 此方法执行有界的同步磁盘读取，异步代码应在阻塞线程上调用。
    pub fn day_page(
        &self,
        day: i32,
        day_id: Id,
        after: u64,
        through: u64,
        budget: usize,
    ) -> Result<Page, QueryError> {
        self.read
            .read()
            .unwrap()
            .day_page(day, day_id, after, through, budget)
    }

    pub fn tail_page(&self, cursor: Option<TailCursor>, budget: usize) -> Result<Page, QueryError> {
        self.read.read().unwrap().tail_page(cursor, budget)
    }

    fn close(&self) {
        let mut pending = self.inbox.pending.lock().unwrap();
        if !pending.closed {
            pending.closed = true;
            pending.flush_dropped();
            pending.queue.push_back(Message::Close);
            self.inbox.ready.notify_one();
        }
    }
}

impl Pending {
    fn flush_dropped(&mut self) {
        if let Some(loss) = self.dropped.take() {
            self.queue.push_back(Message::Overrun(loss));
        }
    }
}

impl Inbox {
    fn recv(&self, timeout: Duration) -> Option<Message> {
        let mut pending = self.pending.lock().unwrap();
        if pending.queue.is_empty() {
            pending = self.ready.wait_timeout(pending, timeout).unwrap().0;
        }
        let next = pending.queue.pop_front();
        if matches!(next, Some(Message::Sample(_))) {
            pending.samples -= 1;
        }
        next
    }
}

impl Recorder {
    pub fn start(dir: PathBuf, options: RecorderOptions) -> io::Result<Self> {
        let handle = RecorderHandle {
            inbox: Arc::new(Inbox {
                pending: Mutex::new(Pending::default()),
                ready: Condvar::new(),
            }),
            read: Arc::new(RwLock::new(ReadState::new(dir.clone()))),
        };
        let (done_tx, done) = mpsc::channel();
        let thread_handle = handle.clone();
        let thread = thread::Builder::new()
            .name("history-recorder".into())
            .spawn(move || {
                let _ = done_tx.send(Worker::new(dir, options, thread_handle).run());
            })?;
        Ok(Self {
            handle,
            done,
            thread: Some(thread),
        })
    }

    pub fn handle(&self) -> RecorderHandle {
        self.handle.clone()
    }

    pub fn shutdown(mut self, timeout: Duration) -> io::Result<()> {
        self.handle.close();
        let result = self.done.recv_timeout(timeout).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => io::Error::new(
                io::ErrorKind::TimedOut,
                "history recorder shutdown timed out",
            ),
            mpsc::RecvTimeoutError::Disconnected => {
                io::Error::other("history recorder stopped unexpectedly")
            }
        })?;
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| io::Error::other("history recorder panicked"))?;
        }
        result
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.handle.close();
    }
}

struct Worker {
    dir: PathBuf,
    options: RecorderOptions,
    handle: RecorderHandle,
    store: Option<Store>,
    retention: Option<Retention>,
    retry_at: Instant,
    sync_at: Instant,
    maintain_at: Instant,
    overrun: Option<Loss>,
    failed: Option<Loss>,
    invalid_time: Option<Loss>,
    failure: Option<String>,
}

impl Worker {
    fn new(dir: PathBuf, options: RecorderOptions, handle: RecorderHandle) -> Self {
        let now = Instant::now();
        Self {
            dir,
            options,
            handle,
            store: None,
            retention: None,
            retry_at: now,
            sync_at: now + SYNC_INTERVAL,
            maintain_at: now,
            overrun: None,
            failed: None,
            invalid_time: None,
            failure: None,
        }
    }

    fn fail(&mut self, error: io::Error) {
        let message = error.to_string();
        if self.failure.as_deref() != Some(&message) {
            tracing::warn!(%error, "history storage unavailable");
        }
        self.failure = Some(message.clone());
        self.handle
            .read
            .write()
            .unwrap()
            .set_storage(StorageState::Unavailable(message));
        if let Some((first, last)) = self.store.as_ref().and_then(Store::unsynced_samples) {
            let later = self.failed.take().map_or(last, |loss| loss.last);
            self.failed = Some(Loss { first, last: later });
        }
        // 保留原有可读视图，直到成功重新打开存储。
        self.store = None;
        self.retention = None;
        self.retry_at = Instant::now() + RETRY_INTERVAL;
    }

    fn open(&mut self) {
        match Store::open(self.dir.clone(), self.handle.read.clone()) {
            Ok(store) => {
                match Retention::open(&self.dir, store.store_id(), store.new_store()) {
                    Ok(retention) => self.retention = Some(retention),
                    Err(error) => tracing::warn!(%error, "unable to open history retention ledger"),
                }
                self.store = Some(store);
                self.failure = None;
                self.maintain_at = Instant::now();
                self.status();
            }
            Err(error) => {
                if self.handle.snapshot().store_id.is_none() {
                    Store::inspect_read_only(&self.dir, &self.handle.read);
                }
                self.fail(error);
            }
        }
    }

    fn status(&self) {
        let mut read = self.handle.read.write().unwrap();
        if let Some(retention) = &self.retention {
            read.set_retention(retention.publish_state());
        }
        if self.failure.is_none() {
            read.set_storage(if self.options.enabled {
                StorageState::Recording
            } else {
                StorageState::Disabled
            });
        }
    }

    fn maintain(&mut self, sample: Option<&Sample>) {
        if let (Some(retention), Some(store)) = (&mut self.retention, &mut self.store) {
            if let Err(error) = retention.maintain(store, &self.options, sample) {
                tracing::warn!(%error, "history retention maintenance failed");
            }
            self.status();
        }
        self.maintain_at = Instant::now() + RETRY_INTERVAL;
    }

    fn background(&mut self) {
        let now = Instant::now();
        if self.store.is_none() && now >= self.retry_at {
            self.open();
        }
        if now >= self.maintain_at {
            self.maintain(None);
        }
        let Some(store) = &mut self.store else { return };
        let result = if store.seal_due(self.options.seal_interval) {
            store.seal(self.options.compression_level)
        } else if now >= self.sync_at {
            self.sync_at = now + SYNC_INTERVAL;
            store.sync_wal()
        } else {
            Ok(())
        };
        if let Err(error) = result {
            self.fail(error);
        }
    }

    fn record(&mut self, sample: Sample) {
        if !self.options.enabled {
            if let Some(store) = &mut self.store {
                if let Err(error) = store.ensure_disabled(&sample, self.options.compression_level) {
                    self.fail(error);
                }
            }
            return;
        }
        if store::sample_day(&sample).is_none() {
            Loss::push(&mut self.invalid_time, &sample);
            return;
        }
        if self.store.as_ref().is_some_and(|s| {
            s.active_day() != store::sample_day(&sample)
                || s.near_capacity(self.options.retention_bytes)
        }) {
            self.maintain(Some(&sample));
        }
        let Some(store) = &mut self.store else {
            Loss::push(&mut self.failed, &sample);
            return;
        };
        let losses = [
            (&self.failed, GapReason::Storage),
            (&self.overrun, GapReason::RecorderOverrun),
            (&self.invalid_time, GapReason::Unknown),
        ];
        let gaps = losses
            .into_iter()
            .filter_map(|(loss, reason)| {
                let loss = loss.as_ref()?;
                let start = loss.start_after(store.latest()).or_else(|| {
                    if matches!(reason, GapReason::Unknown) {
                        store.latest().map(|p| {
                            p.sample
                                .frame
                                .time_ms
                                .saturating_add(i64::from(p.sample.frame.period_ms))
                        })
                    } else {
                        None
                    }
                })?;
                Some((start, reason))
            })
            .collect::<Vec<_>>();
        match store.record(&sample, &gaps, &self.options) {
            Ok(()) => {
                self.failed = None;
                self.overrun = None;
                self.invalid_time = None;
            }
            Err(error) => {
                Loss::push(&mut self.failed, &sample);
                self.fail(error);
            }
        }
    }

    fn configure(&mut self, options: RecorderOptions) {
        if self.options.enabled && !options.enabled {
            if let Some(store) = &mut self.store {
                if let Err(error) = store
                    .flush_losses(
                        options.compression_level,
                        self.overrun.as_ref(),
                        self.failed.as_ref(),
                    )
                    .and_then(|()| store.stop_recording(options.compression_level))
                {
                    self.fail(error);
                } else {
                    self.overrun = None;
                    self.failed = None;
                    self.invalid_time = None;
                }
            }
        }
        self.options = options;
        self.maintain(None);
        self.status();
    }

    fn run(mut self) -> io::Result<()> {
        loop {
            self.background();
            match self.handle.inbox.recv(Duration::from_millis(250)) {
                Some(Message::Sample(sample)) => self.record(sample),
                Some(Message::Overrun(loss)) if self.options.enabled => {
                    Loss::push(&mut self.overrun, &loss.first);
                    Loss::push(&mut self.overrun, &loss.last);
                }
                Some(Message::Configure(options)) => self.configure(options),
                Some(Message::AckClock {
                    store_id,
                    event_id,
                    displayed_wall_ms,
                    issued_at,
                    reply,
                }) => {
                    let result = self
                        .retention
                        .as_mut()
                        .ok_or(ClockAckError::Storage)
                        .and_then(|retention| {
                            retention.ack_clock(store_id, event_id, displayed_wall_ms, issued_at)
                        });
                    if result.is_ok() {
                        self.maintain(None);
                    }
                    self.status();
                    let _ = reply.send(result);
                }
                Some(Message::Close) => {
                    if self.store.is_none() {
                        self.open();
                    }
                    let store = self.store.as_mut().ok_or_else(|| {
                        io::Error::other(
                            self.failure
                                .as_deref()
                                .unwrap_or("history storage unavailable"),
                        )
                    })?;
                    // 已无后续样本，以最后一个丢失样本及其周期限定缺口终点。
                    store.finish(
                        self.options.compression_level,
                        self.overrun.as_ref(),
                        self.failed.as_ref(),
                    )?;
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}
