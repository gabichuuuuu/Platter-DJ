//! The mix recorder on the control side: the worker that empties the queue the processor fills
//! with the master and writes it into WAV files.
//!
//! Behaviour note: `docs/behavior/recording.md`; design: `docs/decisions/0035-mix-recorder.md`.
//! The worker runs on a thread of its own, so the disk never holds up the audio thread or the
//! interface. The control side starts one worker per recording; the worker hands the queue back
//! when it returns, and a new recording waits for it.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use platter_core::SampleRate;
use rtrb::{Consumer, Producer};

use crate::control::{EngineError, Event};
use crate::rt::{Recorded, RtCommand, Shared};
use crate::wav::WavWriter;

/// Gives the path of the first file of a new recording, without its extension. The files that
/// follow a cut or a split add a part number to its name. Set it with
/// [`crate::Engine::set_recording_names`].
pub type RecordingNames = Arc<dyn Fn() -> PathBuf + Send + Sync>;

/// A file is closed and the next one opened at this size of samples by default: the largest
/// split the public manual offers. A WAV file cannot pass 4 GiB.
pub const SPLIT_BYTES: u64 = 2048 * 1024 * 1024;
/// The smallest split [`crate::Engine::set_recording_split`] takes.
pub const MIN_SPLIT_BYTES: u64 = 1024 * 1024;
const BYTES_PER_FRAME: u64 = 8;
/// How long the worker waits, after a stop, for the processor to mark the end. The processor
/// does not run without an output, and then the file is finished anyway.
const END_WAIT: Duration = Duration::from_secs(2);
/// How often the worker looks at the queue when nothing else wakes it. Far below what the
/// queue holds (`rt::RECORDER_QUEUE_SECONDS`).
const POLL: Duration = Duration::from_millis(10);
/// Items taken from the queue at once, so the processor gets room back while the disk works.
const BATCH: usize = 4096;
/// Names tried for one part before the recording gives up.
const MAX_TRIES: u32 = 1000;

/// What the interface shows of the recorder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecorderSnapshot {
    /// A recording runs: Rec is lit and Cut acts.
    pub recording: bool,
    /// Length of the current recording or, after a stop, of the last one.
    pub seconds: f64,
    /// Frames of that recording the disk could not take.
    pub dropped_frames: u64,
    /// The file being written.
    pub file: Option<PathBuf>,
    /// Why the last recording stopped by itself, until the next one starts.
    pub failure: Option<String>,
}

/// What the control side tells a worker.
pub(crate) enum Order {
    Cut,
    Stop,
}

/// What a worker tells the control side.
pub(crate) enum Report {
    Opened(PathBuf),
    Closed { path: PathBuf, frames: u64 },
    Failed(String),
}

struct Worker {
    orders: Sender<Order>,
    reports: Receiver<Report>,
    /// The thread gives the queue back when it ends; `None` if it never got it.
    handle: JoinHandle<Option<Consumer<Recorded>>>,
}

/// The state of the recorder the engine keeps.
pub(crate) struct Recorder {
    /// The queue, while no worker holds it.
    queue: Option<Consumer<Recorded>>,
    worker: Option<Worker>,
    pub(crate) names: Option<RecordingNames>,
    /// The size of samples at which the next recording splits its files.
    pub(crate) split_bytes: u64,
    /// The number of the last recording started.
    number: u32,
    recording: bool,
    /// A worker failed and the stop could not be sent yet.
    stop_wanted: bool,
    file: Option<PathBuf>,
    failure: Option<String>,
}

impl Recorder {
    pub(crate) fn new(queue: Consumer<Recorded>) -> Self {
        Self {
            queue: Some(queue),
            worker: None,
            names: None,
            split_bytes: SPLIT_BYTES,
            number: 0,
            recording: false,
            stop_wanted: false,
            file: None,
            failure: None,
        }
    }

    pub(crate) fn toggle(
        &mut self,
        rate: SampleRate,
        commands: &mut Producer<RtCommand>,
    ) -> Result<(), EngineError> {
        if self.recording {
            self.stop(commands)
        } else {
            self.start(rate, commands)
        }
    }

    fn start(
        &mut self,
        rate: SampleRate,
        commands: &mut Producer<RtCommand>,
    ) -> Result<(), EngineError> {
        let names = self.names.clone().ok_or(EngineError::NoRecordingFolder)?;
        // Until the worker of the last recording gives the queue back, it may still write.
        if self.queue.is_none() || commands.slots() == 0 {
            return Err(EngineError::Busy);
        }
        let number = self.number.wrapping_add(1);
        let (orders, orders_in) = channel();
        let (reports_out, reports) = channel();
        // The queue goes to the thread once it runs, so a thread that cannot start leaves it
        // here.
        let (give, given) = channel::<Consumer<Recorded>>();
        let job = Job {
            names,
            rate,
            number,
            split_frames: split_frames(self.split_bytes),
            orders: orders_in,
            reports: reports_out,
        };
        let handle = std::thread::Builder::new()
            .name("platter-recorder".to_owned())
            .spawn(move || given.recv().ok().map(|queue| run(queue, job)))
            .map_err(EngineError::Worker)?;
        if let Some(queue) = self.queue.take() {
            // The thread waits for it, so the channel is open.
            let _ = give.send(queue);
        }
        self.worker = Some(Worker {
            orders,
            reports,
            handle,
        });
        self.number = number;
        if commands
            .push(RtCommand::Record {
                on: true,
                recording: number,
            })
            .is_err()
        {
            // Checked above: only this side pushes. The worker gives up after its wait.
            self.stop_worker();
            return Err(EngineError::Busy);
        }
        self.recording = true;
        self.stop_wanted = false;
        self.file = None;
        self.failure = None;
        Ok(())
    }

    fn stop(&mut self, commands: &mut Producer<RtCommand>) -> Result<(), EngineError> {
        commands
            .push(RtCommand::Record {
                on: false,
                recording: self.number,
            })
            .map_err(|_| EngineError::Busy)?;
        self.recording = false;
        self.stop_wanted = false;
        self.stop_worker();
        Ok(())
    }

    fn stop_worker(&self) {
        if let Some(worker) = &self.worker {
            // A worker that has ended needs no order.
            let _ = worker.orders.send(Order::Stop);
        }
    }

    pub(crate) fn cut(&mut self) {
        if let (true, Some(worker)) = (self.recording, &self.worker) {
            let _ = worker.orders.send(Order::Cut);
        }
    }

    /// Takes in what the worker reported, stops a recording that failed and takes the queue
    /// back from a worker that ended. `dropped` is the count of frames the processor could
    /// not queue.
    pub(crate) fn poll(
        &mut self,
        commands: &mut Producer<RtCommand>,
        dropped: u64,
        events: &mut Vec<Event>,
    ) {
        let reports: Vec<Report> = self
            .worker
            .as_ref()
            .map(|worker| worker.reports.try_iter().collect())
            .unwrap_or_default();
        for report in reports {
            self.take(report, events);
        }
        if self.stop_wanted && self.recording {
            let _ = self.stop(commands);
        }
        let ended = self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.handle.is_finished());
        if !ended {
            return;
        }
        let Some(worker) = self.worker.take() else {
            return;
        };
        let reports: Vec<Report> = worker.reports.try_iter().collect();
        for report in reports {
            self.take(report, events);
        }
        match worker.handle.join() {
            Ok(Some(queue)) => self.queue = Some(queue),
            // Without its queue the recorder cannot record again in this engine.
            _ => self.failed("the recorder stopped".to_owned(), events),
        }
        if dropped > 0 {
            self.failed(
                format!("{dropped} frames came faster than the disk took them and are missing"),
                events,
            );
        }
        self.recording = false;
        self.file = None;
    }

    fn take(&mut self, report: Report, events: &mut Vec<Event>) {
        match report {
            Report::Opened(path) => self.file = Some(path),
            Report::Closed { path, frames } => {
                if self.file.as_ref() == Some(&path) {
                    self.file = None;
                }
                events.push(Event::Recorded { path, frames });
            }
            Report::Failed(message) => {
                self.failed(message, events);
                self.stop_wanted = self.recording;
            }
        }
    }

    fn failed(&mut self, message: String, events: &mut Vec<Event>) {
        self.failure = Some(message.clone());
        events.push(Event::RecordingFailed { message });
    }

    /// Stops a recording that runs and waits up to `wait` for its file to be finished. The
    /// processor must go on running meanwhile, on another thread, to mark the end.
    pub(crate) fn finish(
        &mut self,
        wait: Duration,
        commands: &mut Producer<RtCommand>,
        shared: &Shared,
        events: &mut Vec<Event>,
    ) {
        if self.recording {
            let _ = self.stop(commands);
        }
        let deadline = Instant::now() + wait;
        loop {
            self.poll(commands, shared.recorder_dropped.get(), events);
            if self.worker.is_none() || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(POLL);
        }
    }

    pub(crate) fn snapshot(&self, shared: &Shared, rate: SampleRate) -> RecorderSnapshot {
        RecorderSnapshot {
            recording: self.recording,
            seconds: shared.recorded_frames.get() as f64 / rate.as_f64(),
            dropped_frames: shared.recorder_dropped.get(),
            file: self.file.clone(),
            failure: self.failure.clone(),
        }
    }
}

/// Everything a worker needs besides the queue.
pub(crate) struct Job {
    pub(crate) names: RecordingNames,
    pub(crate) rate: SampleRate,
    pub(crate) number: u32,
    pub(crate) split_frames: u64,
    pub(crate) orders: Receiver<Order>,
    pub(crate) reports: Sender<Report>,
}

pub(crate) fn split_frames(bytes: u64) -> u64 {
    bytes / BYTES_PER_FRAME
}

/// The body of a worker: it writes the frames of recording `job.number` until their end comes,
/// the control side is gone, or a stop has waited [`END_WAIT`]. Returns the queue.
pub(crate) fn run(mut queue: Consumer<Recorded>, job: Job) -> Consumer<Recorded> {
    let mut writer = Writer::new((job.names)(), job.rate, job.split_frames, job.reports);
    writer.open_next();
    let mut started = false;
    let mut stop_by: Option<Instant> = None;
    loop {
        if drain(&mut queue, job.number, &mut started, &mut writer) {
            break;
        }
        match job.orders.recv_timeout(POLL) {
            Ok(Order::Cut) => {
                // What came before the press goes into the file being closed.
                if drain(&mut queue, job.number, &mut started, &mut writer) {
                    break;
                }
                writer.close();
                writer.open_next();
            }
            Ok(Order::Stop) => stop_by = stop_by.or(Some(Instant::now() + END_WAIT)),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                drain(&mut queue, job.number, &mut started, &mut writer);
                break;
            }
        }
        let late = stop_by.is_some_and(|by| Instant::now() >= by);
        if late || (queue.is_abandoned() && queue.is_empty()) {
            drain(&mut queue, job.number, &mut started, &mut writer);
            break;
        }
    }
    writer.close();
    queue
}

/// Takes what the queue holds, a batch at a time. Returns true once the end of recording
/// `number` came; what follows it stays in the queue.
fn drain(
    queue: &mut Consumer<Recorded>,
    number: u32,
    started: &mut bool,
    writer: &mut Writer,
) -> bool {
    // Bounded by what the queue held when the call started.
    let mut left = queue.slots();
    while left > 0 {
        let take = left.min(BATCH);
        left -= take;
        let Ok(chunk) = queue.read_chunk(take) else {
            return false;
        };
        for item in chunk {
            match item {
                Recorded::Start(of) if of == number => *started = true,
                Recorded::End(of) if of == number => return true,
                Recorded::Frame(frame) if *started => writer.write(frame),
                // Frames and marks another recording left behind.
                _ => {}
            }
        }
    }
    false
}

struct Part {
    path: PathBuf,
    wav: WavWriter<BufWriter<File>>,
    frames: u64,
}

/// The files of one recording.
struct Writer {
    /// The path of the first part, without its extension.
    stem: PathBuf,
    rate: SampleRate,
    split_frames: u64,
    /// The part number the next file tries first.
    next: u32,
    current: Option<Part>,
    /// A problem stopped the writing: what comes next is dropped.
    failed: bool,
    reports: Sender<Report>,
}

impl Writer {
    fn new(stem: PathBuf, rate: SampleRate, split_frames: u64, reports: Sender<Report>) -> Self {
        Self {
            stem,
            rate,
            split_frames: split_frames.max(1),
            next: 1,
            current: None,
            failed: false,
            reports,
        }
    }

    /// The path of part `number`: the stem for the first, the stem and the number after it.
    fn path_of(&self, number: u32) -> Option<PathBuf> {
        let mut name: OsString = self.stem.file_name()?.to_os_string();
        if number > 1 {
            name.push(format!("-{number}"));
        }
        name.push(".wav");
        Some(self.stem.with_file_name(name))
    }

    fn open_next(&mut self) {
        if self.failed {
            return;
        }
        match self.create() {
            Ok(part) => {
                // The control side may be gone; then nobody waits for the report.
                let _ = self.reports.send(Report::Opened(part.path.clone()));
                self.current = Some(part);
            }
            Err(error) => {
                let stem = self.stem.clone();
                self.fail(&stem, &error);
            }
        }
    }

    fn create(&mut self) -> io::Result<Part> {
        let no_name = || io::Error::new(ErrorKind::InvalidInput, "a recording needs a file name");
        if let Some(folder) = self
            .stem
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty())
        {
            std::fs::create_dir_all(folder)?;
        }
        for _ in 0..MAX_TRIES {
            let number = self.next;
            self.next = self.next.saturating_add(1);
            let path = self.path_of(number).ok_or_else(no_name)?;
            // A file is never written over.
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    let wav = WavWriter::new(BufWriter::new(file), self.rate, 2)?;
                    return Ok(Part {
                        path,
                        wav,
                        frames: 0,
                    });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            ErrorKind::AlreadyExists,
            "every name tried for the recording is in use",
        ))
    }

    fn write(&mut self, frame: [f32; 2]) {
        if self
            .current
            .as_ref()
            .is_some_and(|part| part.frames >= self.split_frames)
        {
            self.close();
            self.open_next();
        }
        let Some(part) = self.current.as_mut() else {
            return;
        };
        match part.wav.write(&frame) {
            Ok(()) => part.frames += 1,
            Err(error) => {
                let path = part.path.clone();
                self.fail(&path, &error);
            }
        }
    }

    /// Finishes the file being written: its header gets the sizes.
    fn close(&mut self) {
        let Some(part) = self.current.take() else {
            return;
        };
        match part.wav.finish() {
            Ok(_) => {
                let _ = self.reports.send(Report::Closed {
                    path: part.path,
                    frames: part.frames,
                });
            }
            Err(error) => self.fail(&part.path, &error),
        }
    }

    fn fail(&mut self, path: &Path, error: &io::Error) {
        if let Some(part) = self.current.take() {
            // What was written stays readable as far as the disk lets the header be fixed.
            let _ = part.wav.finish();
        }
        self.failed = true;
        let _ = self
            .reports
            .send(Report::Failed(format!("{}: {error}", path.display())));
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::mpsc::channel;

    use platter_core::SampleRate;
    use platter_decode::decode_file;
    use rtrb::RingBuffer;

    use super::{Job, Report, run};
    use crate::rt::Recorded;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("platter-recorder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        folder
    }

    /// Runs a worker over what `items` holds, as if the processor had sent it, and returns its
    /// reports.
    fn record(folder: &Path, items: &[Recorded], number: u32, split_frames: u64) -> Vec<Report> {
        let (mut producer, consumer) = RingBuffer::new(items.len() + 1);
        for item in items {
            producer.push(*item).unwrap();
        }
        let (orders_sender, orders) = channel();
        let (reports, received) = channel();
        let stem = folder.join("take");
        let job = Job {
            names: Arc::new(move || stem.clone()),
            rate: SampleRate::HZ_48000,
            number,
            split_frames,
            orders,
            reports,
        };
        let _ = run(consumer, job);
        drop(orders_sender);
        received.try_iter().collect()
    }

    fn closed(reports: &[Report]) -> Vec<(String, u64)> {
        reports
            .iter()
            .filter_map(|report| match report {
                Report::Closed { path, frames } => {
                    Some((path.file_name()?.to_string_lossy().into_owned(), *frames))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_split_opens_the_next_part_without_losing_a_frame() {
        let folder = folder("split");
        let frames: Vec<[f32; 2]> = (0..10)
            .map(|n| [n as f32 * 0.01, -(n as f32) * 0.01])
            .collect();
        let mut items = vec![Recorded::Start(3)];
        items.extend(frames.iter().map(|frame| Recorded::Frame(*frame)));
        items.push(Recorded::End(3));
        let reports = record(&folder, &items, 3, 4);
        assert_eq!(
            closed(&reports),
            [
                ("take.wav".to_owned(), 4),
                ("take-2.wav".to_owned(), 4),
                ("take-3.wav".to_owned(), 2)
            ]
        );
        let mut heard = Vec::new();
        for name in ["take.wav", "take-2.wav", "take-3.wav"] {
            heard.extend(decode_file(&folder.join(name)).unwrap().samples);
        }
        let sent: Vec<f32> = frames.iter().flatten().copied().collect();
        assert_eq!(heard, sent);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn what_another_recording_left_is_skipped_and_no_file_is_written_over() {
        let folder = folder("skip");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("take.wav"), b"kept").unwrap();
        let items = [
            Recorded::Frame([0.5, 0.5]),
            Recorded::End(1),
            Recorded::Start(2),
            Recorded::Frame([0.25, -0.25]),
            Recorded::End(2),
        ];
        let reports = record(&folder, &items, 2, 100);
        assert_eq!(closed(&reports), [("take-2.wav".to_owned(), 1)]);
        assert_eq!(std::fs::read(folder.join("take.wav")).unwrap(), b"kept");
        let audio = decode_file(&folder.join("take-2.wav")).unwrap();
        assert_eq!(audio.samples, [0.25, -0.25]);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
