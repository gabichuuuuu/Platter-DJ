//! Key lock on worker threads: the stretches of the decks render ahead on threads of their own,
//! raised to the priority of audio, and the audio thread only takes what they rendered.
//!
//! Behaviour note: `docs/behavior/keylock.md`, *Worker threads*; the choice and its
//! measurement: `docs/decisions/0039-key-lock-threads.md`. This is the only module that names
//! audio_thread_priority.
//!
//! A [`Remote`] stands for a stretch on the audio thread and implements [`Stretch`] as the
//! stretch itself does, so the key lock of a deck does not change. A change of tempo or pitch is
//! stamped with the frame of output it applies at, a fixed lead after the frame being played,
//! and asked on frames chosen by the position in the track, so the four stretches of the stems
//! of a deck, each on whatever worker, change on the same frame and stay together.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use platter_core::SampleRate;
use platter_dsp::Frame;
use platter_dsp::stretch::Stretch;
use rtrb::{Consumer, Producer, RingBuffer};

use crate::rt::TrackAudio;

/// Output rendered ahead of what plays. Twice the latest wake-up measured on a worker raised to
/// the priority of audio with every processor busy (5 ms), plus a block of the device and the
/// time of a pass.
const RESERVE_SECONDS: f64 = 0.016;
/// How far beyond the reserve a change of tempo or pitch applies: the worker has rendered at
/// most the reserve when it reads the change, so the frame it applies at is still to come.
const LEAD_SECONDS: f64 = 0.001;
/// Requests a stretch may have waiting: seeks and changes of tempo or pitch. At one change for
/// every [`CHANGE_FRAMES`] frames of the track, a worker would have to stop for a third of a
/// second to fill it.
const REQUESTS: usize = 256;
/// Frames of the track between two changes of tempo or pitch. A deck glides its tempo, so the
/// tempo it asks changes on every frame; the stretch itself schedules a change once a chunk of
/// 64 frames. Asked when the position in the track crosses a multiple of this, a change is
/// asked on the same frame by every stretch of a deck, even by one that started again alone.
const CHANGE_FRAMES: f64 = 64.0;
/// Changes asked and not yet heard a stretch keeps, for a seek to ask them again: at a tempo
/// of 4, one is asked every 16 frames of output, about 51 within the lead.
const AHEAD: usize = 64;
/// Stretches waiting to be taken by a worker.
const ARRIVING: usize = 32;
/// Steps of work after a seek one stretch takes in one pass of a worker; a seek takes about
/// 310. A worker tops up every stretch that plays before it prepares one that seeks, so the
/// stretches of a deck that all seek at once do not starve the others.
const PREPARE_STEPS: usize = 128;
/// Rest of a worker between two passes.
const REST: Duration = Duration::from_millis(1);
/// Workers at most, and processors for each.
const MAX_WORKERS: usize = 4;
const PROCESSORS_PER_WORKER: usize = 4;
/// Smallest change of tempo or pitch worth a request.
const EPSILON: f64 = 1e-9;
/// The period a worker declares to the system, which macOS uses for its share of time.
const PERIOD_SECONDS: f64 = 0.002;
/// How long the start waits for each worker to say whether it runs at the priority of audio.
const RAISE_WAIT: Duration = Duration::from_secs(1);

enum Request {
    Seek {
        epoch: u32,
        frame: f64,
        tempo: f64,
        pitch: f64,
    },
    Change {
        epoch: u32,
        /// The frame of output since the seek that plays at the new tempo and pitch first.
        at: u64,
        tempo: f64,
        pitch: f64,
    },
}

#[derive(Clone, Copy)]
struct Rendered {
    epoch: u32,
    frame: Frame,
    position: f64,
}

/// The stretch of a track as the audio thread sees it, while a worker runs it.
pub struct Remote {
    requests: Producer<Request>,
    rendered: Consumer<Rendered>,
    /// Counts the seeks, so what was rendered before the last one is told apart.
    epoch: u32,
    /// Frames taken since the last seek.
    taken: u64,
    /// The position of the frame taken last, for the boundaries of the changes.
    last: f64,
    /// Tempo and pitch last asked of the worker.
    tempo: f64,
    pitch: f64,
    /// Tempo and pitch of the frame taken last.
    heard: (f64, f64),
    /// Changes asked and not yet heard, in order: the frame since the seek each applies at,
    /// with its tempo and pitch. A fixed ring, so asking never allocates.
    ahead: [(u64, f64, f64); AHEAD],
    ahead_start: usize,
    ahead_len: usize,
    /// A frame was taken since the stretch was made.
    started: bool,
    lead: u64,
    /// A seek the queue of requests had no room for yet.
    pending: Option<(f64, f64, f64)>,
    /// Counts the frames asked while playing that the worker had not rendered in time.
    starved: Arc<AtomicUsize>,
    /// Keeps the workers running while the stretch plays, even once the engine stopped handing
    /// new stretches to them.
    _pool: Option<Arc<StretchPool>>,
}

impl Remote {
    /// Sends the seek that waits, then the changes asked before it and not yet heard, so the
    /// stretch plays from the seek through the same changes as the other stretches of its deck.
    fn send_seek(&mut self) {
        let Some((frame, tempo, pitch)) = self.pending else {
            return;
        };
        let seek = Request::Seek {
            epoch: self.epoch,
            frame,
            tempo,
            pitch,
        };
        if self.requests.push(seek).is_err() {
            return;
        }
        self.pending = None;
        for index in 0..self.ahead_len {
            let (at, tempo, pitch) = self.ahead[(self.ahead_start + index) % AHEAD];
            let change = Request::Change {
                epoch: self.epoch,
                at,
                tempo,
                pitch,
            };
            if self.requests.push(change).is_err() {
                break;
            }
        }
    }

    /// Drops what was rendered before the last seek: at most the size of the queue.
    fn drop_stale(&mut self) {
        while let Ok(rendered) = self.rendered.peek() {
            if rendered.epoch == self.epoch {
                break;
            }
            let _ = self.rendered.pop();
        }
    }

    /// The changes that apply at frame `frame` since the seek, or before, are heard from it.
    fn hear_until(&mut self, frame: u64) {
        while self.ahead_len > 0 {
            let (at, tempo, pitch) = self.ahead[self.ahead_start];
            if at > frame {
                break;
            }
            self.heard = (tempo, pitch);
            self.ahead_start = (self.ahead_start + 1) % AHEAD;
            self.ahead_len -= 1;
        }
    }
}

impl Stretch for Remote {
    fn seek(&mut self, frame: f64, tempo: f64, pitch: f64) {
        if self.started {
            // It starts again at the tempo the other stretches of its deck play now, and goes
            // through the changes they still have to hear, on the same frames.
            self.hear_until(self.taken);
            let taken = self.taken;
            for index in 0..self.ahead_len {
                let slot = &mut self.ahead[(self.ahead_start + index) % AHEAD];
                slot.0 -= taken;
            }
        } else {
            self.heard = (tempo, pitch);
            self.tempo = tempo;
            self.pitch = pitch;
        }
        self.epoch = self.epoch.wrapping_add(1);
        self.taken = 0;
        self.last = f64::NAN;
        let (tempo, pitch) = self.heard;
        // A seek that waits is replaced by the next: only the last one counts.
        self.pending = Some((frame, tempo, pitch));
        self.send_seek();
    }

    fn prepare(&mut self, _: &[f32]) -> bool {
        self.send_seek();
        if self.pending.is_some() {
            return false;
        }
        self.drop_stale();
        self.rendered.peek().is_ok()
    }

    fn next(&mut self, _: &[f32], tempo: f64, pitch: f64) -> Option<(Frame, f64)> {
        self.send_seek();
        if self.pending.is_some() {
            return None;
        }
        self.drop_stale();
        let Ok(rendered) = self.rendered.pop() else {
            if self.taken > 0 {
                self.starved.fetch_add(1, Ordering::Relaxed);
            }
            return None;
        };
        self.started = true;
        self.hear_until(self.taken);
        let crossed =
            (rendered.position / CHANGE_FRAMES).floor() != (self.last / CHANGE_FRAMES).floor();
        let changed = (tempo - self.tempo).abs() > EPSILON || (pitch - self.pitch).abs() > EPSILON;
        // A full ring or queue keeps the tempo before; the next boundary asks again.
        if changed && crossed && self.ahead_len < AHEAD {
            let at = self.taken + self.lead;
            let change = Request::Change {
                epoch: self.epoch,
                at,
                tempo,
                pitch,
            };
            if self.requests.push(change).is_ok() {
                self.ahead[(self.ahead_start + self.ahead_len) % AHEAD] = (at, tempo, pitch);
                self.ahead_len += 1;
                self.tempo = tempo;
                self.pitch = pitch;
            }
        }
        self.taken += 1;
        self.last = rendered.position;
        Some((rendered.frame, rendered.position))
    }
}

/// A stretch and its track on a worker, with the ends of the queues of its [`Remote`].
struct Job {
    stretch: Box<dyn Stretch>,
    track: TrackAudio,
    requests: Consumer<Request>,
    rendered: Producer<Rendered>,
    reserve: usize,
    epoch: u32,
    /// Frames rendered since the last seek.
    made: u64,
    tempo: f64,
    pitch: f64,
    changes: VecDeque<(u64, f64, f64)>,
    /// A seek was asked, and the stretch has done the work for it.
    seeking: bool,
    ready: bool,
}

impl Job {
    fn abandoned(&self) -> bool {
        self.requests.is_abandoned()
    }

    /// Takes the waiting requests. Of several seeks only the last is done, with the changes
    /// asked after it.
    fn take_requests(&mut self) {
        let mut seek = None;
        while let Ok(request) = self.requests.pop() {
            match request {
                Request::Seek {
                    epoch,
                    frame,
                    tempo,
                    pitch,
                } => {
                    seek = Some((frame, tempo, pitch));
                    self.epoch = epoch;
                    self.changes.clear();
                }
                Request::Change {
                    epoch,
                    at,
                    tempo,
                    pitch,
                } if epoch == self.epoch => self.changes.push_back((at, tempo, pitch)),
                Request::Change { .. } => {}
            }
        }
        if let Some((frame, tempo, pitch)) = seek {
            self.stretch.seek(frame, tempo, pitch);
            self.made = 0;
            self.tempo = tempo;
            self.pitch = pitch;
            self.seeking = true;
            self.ready = false;
        }
    }

    /// Does a share of the work after a seek, and renders once it is done.
    fn prepare(&mut self) {
        self.take_requests();
        if !self.seeking || self.ready {
            return;
        }
        for _ in 0..PREPARE_STEPS {
            if self.stretch.prepare(self.track.samples()) {
                self.ready = true;
                self.top_up();
                return;
            }
        }
    }

    /// Renders until the reserve is full, or the stretch has nothing more to give.
    fn top_up(&mut self) {
        self.take_requests();
        if !self.seeking || !self.ready {
            return;
        }
        let capacity = self.rendered.buffer().capacity();
        while capacity - self.rendered.slots() < self.reserve {
            self.take_requests();
            if !self.ready {
                break;
            }
            while let Some(&(at, tempo, pitch)) = self.changes.front() {
                if at > self.made {
                    break;
                }
                self.tempo = tempo;
                self.pitch = pitch;
                self.changes.pop_front();
            }
            let Some((frame, position)) =
                self.stretch
                    .next(self.track.samples(), self.tempo, self.pitch)
            else {
                break;
            };
            let rendered = Rendered {
                epoch: self.epoch,
                frame,
                position,
            };
            if self.rendered.push(rendered).is_err() {
                break;
            }
            self.made += 1;
        }
    }
}

/// One pass of a worker over its stretches: every one that plays is topped up before any one
/// that seeks does a share of its work.
fn pass(jobs: &mut Vec<Job>) {
    jobs.retain(|job| !job.abandoned());
    for job in jobs.iter_mut() {
        job.top_up();
    }
    for job in jobs.iter_mut() {
        job.prepare();
    }
}

/// A stretch and the [`Remote`] that stands for it, joined by their queues.
fn pair(
    stretch: Box<dyn Stretch>,
    track: TrackAudio,
    engine_rate: SampleRate,
    starved: Arc<AtomicUsize>,
) -> (Remote, Job) {
    let rate = engine_rate.as_f64();
    let reserve = (RESERVE_SECONDS * rate).ceil() as usize;
    let lead = (reserve as f64 + LEAD_SECONDS * rate).ceil() as u64;
    let (requests, waiting) = RingBuffer::new(REQUESTS);
    let (made, rendered) = RingBuffer::new(reserve * 2);
    let remote = Remote {
        requests,
        rendered,
        epoch: 0,
        taken: 0,
        last: f64::NAN,
        tempo: 1.0,
        pitch: 1.0,
        heard: (1.0, 1.0),
        ahead: [(0, 1.0, 1.0); AHEAD],
        ahead_start: 0,
        ahead_len: 0,
        started: false,
        lead,
        pending: None,
        starved,
        _pool: None,
    };
    let job = Job {
        stretch,
        track,
        requests: waiting,
        rendered: made,
        reserve,
        epoch: 0,
        made: 0,
        tempo: 1.0,
        pitch: 1.0,
        changes: VecDeque::new(),
        seeking: false,
        ready: false,
    };
    (remote, job)
}

struct Worker {
    arriving: Mutex<Producer<Job>>,
    thread: Option<JoinHandle<()>>,
}

/// Where the stretches run: on worker threads, or, for the tests, in [`StretchPool::pump`].
enum Runner {
    Threads(Vec<Worker>),
    Pumped(Mutex<Vec<Job>>),
}

/// The workers the stretches of the decks run on.
pub struct StretchPool {
    runner: Runner,
    next: AtomicUsize,
    stop: Arc<AtomicBool>,
    /// Workers that run at the priority of audio; one that could not runs at the normal
    /// priority, where it may fall behind under load.
    raised: usize,
    starved: Arc<AtomicUsize>,
    rate: SampleRate,
}

impl StretchPool {
    /// Starts the workers, one for every four processors and at most four, and waits until
    /// each says whether it runs at the priority of audio.
    pub fn start(rate: SampleRate) -> std::io::Result<Self> {
        let processors = std::thread::available_parallelism().map_or(1, |count| count.get());
        let count = (processors / PROCESSORS_PER_WORKER).clamp(1, MAX_WORKERS);
        let stop = Arc::new(AtomicBool::new(false));
        let period = (PERIOD_SECONDS * rate.as_f64()).round() as u32;
        let (said, hear) = channel();
        let mut workers = Vec::with_capacity(count);
        for index in 0..count {
            let (arriving, mut coming) = RingBuffer::<Job>::new(ARRIVING);
            let stop = Arc::clone(&stop);
            let said = said.clone();
            let hz = rate.hz();
            let thread = std::thread::Builder::new()
                .name(format!("platter-stretch-{index}"))
                .spawn(move || {
                    let handle =
                        audio_thread_priority::promote_current_thread_to_real_time(period, hz).ok();
                    let _ = said.send(handle.is_some());
                    let mut jobs: Vec<Job> = Vec::new();
                    while !stop.load(Ordering::Relaxed) {
                        while let Ok(job) = coming.pop() {
                            jobs.push(job);
                        }
                        pass(&mut jobs);
                        std::thread::sleep(REST);
                    }
                    if let Some(handle) = handle {
                        let _ = audio_thread_priority::demote_current_thread_from_real_time(handle);
                    }
                })?;
            workers.push(Worker {
                arriving: Mutex::new(arriving),
                thread: Some(thread),
            });
        }
        let raised = (0..count)
            .filter(|_| hear.recv_timeout(RAISE_WAIT).unwrap_or(false))
            .count();
        Ok(Self {
            runner: Runner::Threads(workers),
            next: AtomicUsize::new(0),
            stop,
            raised,
            starved: Arc::new(AtomicUsize::new(0)),
            rate,
        })
    }

    /// A pool with no thread, whose stretches run only when [`Self::pump`] is called: for tests
    /// that run faster than real time.
    pub fn pumped(rate: SampleRate) -> Self {
        Self {
            runner: Runner::Pumped(Mutex::new(Vec::new())),
            next: AtomicUsize::new(0),
            stop: Arc::new(AtomicBool::new(false)),
            raised: 0,
            starved: Arc::new(AtomicUsize::new(0)),
            rate,
        }
    }

    /// Runs one pass over the stretches of a pool built by [`Self::pumped`].
    pub fn pump(&self) {
        if let Runner::Pumped(jobs) = &self.runner
            && let Ok(mut jobs) = jobs.lock()
        {
            pass(&mut jobs);
        }
    }

    /// The workers that run at the priority of audio, of all of them.
    pub fn raised(&self) -> (usize, usize) {
        match &self.runner {
            Runner::Threads(workers) => (self.raised, workers.len()),
            Runner::Pumped(_) => (0, 0),
        }
    }

    /// The frames key lock asked while playing that a worker had not rendered in time, since
    /// the start, the ends of the tracks included: each one sent its deck back to the plain read
    /// of the track for a moment.
    pub fn starved(&self) -> usize {
        self.starved.load(Ordering::Relaxed)
    }

    /// Hands `stretch` of `track` to a worker and returns what stands for it on the audio
    /// thread; when no worker takes it, the stretch itself, to run on the audio thread.
    pub fn adopt(
        self: &Arc<Self>,
        stretch: Box<dyn Stretch>,
        track: &TrackAudio,
    ) -> Box<dyn Stretch> {
        let (mut remote, job) = pair(stretch, track.clone(), self.rate, Arc::clone(&self.starved));
        remote._pool = Some(Arc::clone(self));
        match &self.runner {
            Runner::Threads(workers) => {
                let count = workers.len();
                if count == 0 {
                    return job.stretch;
                }
                let index = self.next.fetch_add(1, Ordering::Relaxed) % count;
                let Ok(mut arriving) = workers[index].arriving.lock() else {
                    return job.stretch;
                };
                match arriving.push(job) {
                    Ok(()) => Box::new(remote),
                    Err(rtrb::PushError::Full(job)) => job.stretch,
                }
            }
            Runner::Pumped(jobs) => match jobs.lock() {
                Ok(mut jobs) => {
                    jobs.push(job);
                    Box::new(remote)
                }
                Err(_) => job.stretch,
            },
        }
    }
}

impl Drop for StretchPool {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Runner::Threads(workers) = &mut self.runner {
            for worker in workers {
                if let Some(thread) = worker.thread.take() {
                    let _ = thread.join();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]

    use super::*;

    /// A stretch that reads the track at the nearest frame and moves by the tempo.
    struct Fake {
        position: f64,
    }

    impl Stretch for Fake {
        fn seek(&mut self, frame: f64, _: f64, _: f64) {
            self.position = frame;
        }
        fn prepare(&mut self, _: &[f32]) -> bool {
            true
        }
        fn next(&mut self, track: &[f32], tempo: f64, _: f64) -> Option<(Frame, f64)> {
            let at = self.position;
            let index = at.round() as usize * 2;
            let frame = [*track.get(index)?, *track.get(index + 1)?];
            self.position += tempo;
            Some((frame, at))
        }
    }

    const RATE: u32 = 48_000;

    fn track() -> TrackAudio {
        let samples: Vec<f32> = (0..RATE as usize * 8)
            .map(|index| (index / 2) as f32)
            .collect();
        TrackAudio::new(SampleRate::new(RATE).unwrap(), samples)
    }

    fn remote() -> (Remote, Job) {
        pair(
            Box::new(Fake { position: 0.0 }),
            track(),
            SampleRate::new(RATE).unwrap(),
            Arc::new(AtomicUsize::new(0)),
        )
    }

    fn pass_one(job: &mut Job) {
        job.top_up();
        job.prepare();
    }

    #[test]
    fn a_remote_plays_what_its_worker_renders_from_the_seek() {
        let (mut remote, mut job) = remote();
        remote.seek(100.0, 1.0, 1.0);
        assert!(!remote.prepare(&[]), "nothing before the worker runs");
        pass_one(&mut job);
        assert!(remote.prepare(&[]));
        for expected in 100..200 {
            let (frame, at) = remote.next(&[], 1.0, 1.0).unwrap();
            assert_eq!((frame[0], at), (expected as f32, f64::from(expected)));
        }
    }

    /// Plays four stretches through a sweep of tempo, their workers running at moments of their
    /// own, and returns the positions of each; `again` starts the second one again alone at
    /// that frame, where it is, as a key lock does after its stretch fell behind.
    fn sweep(again: Option<u64>) -> Vec<Vec<f64>> {
        let mut stems: Vec<(Remote, Job)> = (0..4).map(|_| remote()).collect();
        for (remote, _) in &mut stems {
            remote.seek(0.0, 1.0, 1.0);
        }
        let mut positions = vec![Vec::new(); 4];
        for frame in 0..6_000u64 {
            // A tempo that glides, as the tempo of a deck does.
            let tempo = 1.0 + 0.02 * (frame as f64 / 6_000.0);
            for (index, (_, job)) in stems.iter_mut().enumerate() {
                if frame.is_multiple_of(index as u64 + 3) {
                    pass_one(job);
                }
            }
            if Some(frame) == again {
                let at = *positions[1].last().unwrap() + tempo;
                stems[1].0.seek(at, tempo, 1.0);
            }
            for (index, (remote, job)) in stems.iter_mut().enumerate() {
                let played = remote.next(&[], tempo, 1.0).or_else(|| {
                    pass_one(job);
                    remote.next(&[], tempo, 1.0)
                });
                positions[index].push(played.unwrap().1);
            }
        }
        positions
    }

    /// Four stretches on workers that run in any order change tempo on the same frame.
    #[test]
    fn a_change_of_tempo_reaches_every_stem_on_the_same_frame() {
        let positions = sweep(None);
        assert!(positions.iter().all(|stem| *stem == positions[0]));
    }

    /// A stretch that started again alone still changes tempo on the frames of the others.
    #[test]
    fn a_stem_that_started_again_alone_stays_with_the_others() {
        let positions = sweep(Some(2_000));
        let apart = positions[0]
            .iter()
            .zip(&positions[1])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(apart < 0.5, "{apart} frames apart");
    }

    /// A change asked on a frame plays the lead after it.
    #[test]
    fn a_change_of_tempo_plays_the_lead_later() {
        let (mut remote, mut job) = remote();
        remote.seek(0.0, 1.0, 1.0);
        let mut positions = Vec::new();
        for frame in 0..3_000u64 {
            let tempo = if frame >= 1_000 { 1.5 } else { 1.0 };
            let played = remote.next(&[], tempo, 1.0).or_else(|| {
                pass_one(&mut job);
                remote.next(&[], tempo, 1.0)
            });
            positions.push(played.unwrap().1);
            pass_one(&mut job);
        }
        let first = positions
            .windows(2)
            .position(|pair| pair[1] - pair[0] > 1.0)
            .unwrap() as u64;
        // Asked on the frame where the position crosses the next multiple of 64.
        let asked = 1_024;
        assert_eq!(first, asked + remote.lead);
    }

    /// Seeks asked faster than the worker takes them never block, and the last one wins.
    #[test]
    fn many_seeks_leave_the_last_one() {
        let (mut remote, mut job) = remote();
        for frame in 0..(REQUESTS * 3) {
            remote.seek(frame as f64, 1.0, 1.0);
            if frame % 50 == 0 {
                pass_one(&mut job);
            }
        }
        pass_one(&mut job);
        // The last seek waited for room in the queue: it goes now, and the worker takes it.
        assert!(!remote.prepare(&[]));
        pass_one(&mut job);
        let last = (REQUESTS * 3 - 1) as f64;
        assert_eq!(remote.next(&[], 1.0, 1.0).unwrap().1, last);
    }

    #[test]
    fn a_job_ends_when_its_remote_is_dropped() {
        let pool = Arc::new(StretchPool::pumped(SampleRate::new(RATE).unwrap()));
        let audio = track();
        let stretch = pool.adopt(Box::new(Fake { position: 0.0 }), &audio);
        let count = || match &pool.runner {
            Runner::Pumped(jobs) => jobs.lock().unwrap().len(),
            Runner::Threads(_) => 0,
        };
        assert_eq!(count(), 1);
        drop(stretch);
        pool.pump();
        assert_eq!(count(), 0);
    }

    #[test]
    fn the_pool_runs_a_stretch_on_a_worker_while_its_stretch_lives() {
        let pool = Arc::new(StretchPool::start(SampleRate::new(RATE).unwrap()).unwrap());
        let audio = track();
        let mut stretch = pool.adopt(Box::new(Fake { position: 0.0 }), &audio);
        // The engine lets the pool go; the stretch keeps its worker.
        drop(pool);
        stretch.seek(10.0, 1.0, 1.0);
        let start = std::time::Instant::now();
        while !stretch.prepare(&[]) {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "the worker never rendered"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(stretch.next(&[], 1.0, 1.0).unwrap().1, 10.0);
    }
}
