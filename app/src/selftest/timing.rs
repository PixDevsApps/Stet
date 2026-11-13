//! Self-test commands that wait and measure: for background work, painted frames, time or an
//! assertion to pass; main-loop blocks, load timings, memory and typing latency.

use super::registry::{Command, MANY, commands};
use super::{Harness, StepResult, WAIT_TIMEOUT, broadway, build, number};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// How often the heartbeat looks for main-loop blocks (as in spike S1).
const HEARTBEAT: Duration = Duration::from_millis(5);

pub(super) const COMMANDS: &[Command] = commands![
    "wait-idle" (0, 0) => async wait_for_idle,
    "wait-frames" (1, 1) => async wait_for_frames,
    "sleep" (1, 1) => async sleep,
    "wait-until" (1, MANY) => async wait_until,
    "unpace-frames" (0, 0) => unpace_frames,
    "heartbeat-start" (0, 0) => heartbeat_start,
    "assert-max-block" (1, 1) => assert_max_block,
    "report-memory" (1, 1) => report_memory,
    "report-load" (0, 0) => report_load,
    "assert-first-screen" (1, 2) => assert_first_screen,
    "type-burst" (3, 3) => async type_burst,
];

/// The longest gap between two runs of a 5 ms timer: how long the main loop was blocked.
pub(super) struct Heartbeat {
    last: Cell<Instant>,
    longest: Cell<Duration>,
}

impl Heartbeat {
    fn start() -> Rc<Self> {
        let heartbeat = Rc::new(Self {
            last: Cell::new(Instant::now()),
            longest: Cell::new(Duration::ZERO),
        });
        let weak = Rc::downgrade(&heartbeat);
        glib::timeout_add_local(HEARTBEAT, move || {
            let Some(heartbeat) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let now = Instant::now();
            heartbeat
                .longest
                .set(heartbeat.longest.get().max(now - heartbeat.last.get()));
            heartbeat.last.set(now);
            glib::ControlFlow::Continue
        });
        heartbeat
    }

    fn longest(&self) -> Duration {
        self.longest.get().max(self.last.get().elapsed())
    }
}

impl Harness {
    /// `wait-idle`: waits until spawned tasks have started, workers have finished and no tab
    /// is loading, or until a dialog waits for an answer.
    async fn wait_for_idle(&self, _: &[String]) -> StepResult {
        self.wait_idle().await
    }

    /// `wait-frames <n>`: waits until the window has painted n frames.
    async fn wait_for_frames(&self, args: &[String]) -> StepResult {
        self.wait_frames(number(&args[0])?).await
    }

    /// `sleep <ms>`: waits that long.
    async fn sleep(&self, args: &[String]) -> StepResult {
        glib::timeout_future(Duration::from_millis(number(&args[0])? as u64)).await;
        Ok(())
    }

    /// `wait-until <assertion> <arguments…>`: repeats the assertion every 20 ms until it
    /// passes, for up to 15 s.
    async fn wait_until(&self, args: &[String]) -> StepResult {
        let started = Instant::now();
        loop {
            match self.run_command(&args[0], &args[1..]).await {
                Ok(()) => return Ok(()),
                Err(error) if started.elapsed() > WAIT_TIMEOUT => {
                    return Err(format!("still failing after {WAIT_TIMEOUT:?}: {error}"));
                }
                Err(_) => glib::timeout_future(Duration::from_millis(20)).await,
            }
        }
    }

    /// `unpace-frames`: connects a Broadway web client, so that frames are painted at once
    /// instead of once a second (for timings), and stops the caret blinking.
    #[expect(
        clippy::unused_self,
        reason = "the registry calls every handler as a method"
    )]
    fn unpace_frames(&self, _: &[String]) -> StepResult {
        if let Some(settings) = gtk::Settings::default() {
            settings.set_gtk_cursor_blink(false);
        }
        broadway::connect().map_err(|error| format!("Broadway web client: {error}"))
    }

    /// `heartbeat-start`: starts measuring main-loop blocks for `assert-max-block`.
    fn heartbeat_start(&self, _: &[String]) {
        self.heartbeat.replace(Some(Heartbeat::start()));
    }

    /// `assert-max-block <ms>`: the main loop was never blocked longer since
    /// `heartbeat-start`.
    fn assert_max_block(&self, args: &[String]) -> StepResult {
        let heartbeat = self
            .heartbeat
            .borrow()
            .clone()
            .ok_or("run heartbeat-start first")?;
        let longest = heartbeat.longest();
        let limit = Duration::from_millis(number(&args[0])? as u64);
        println!(
            "  # longest main-loop block: {:.1} ms",
            longest.as_secs_f64() * 1e3
        );
        if longest <= limit {
            Ok(())
        } else {
            Err(format!(
                "the main loop was blocked for {longest:?}, over {limit:?}"
            ))
        }
    }

    /// `report-memory <label>`: prints this process's `VmRSS` and `VmHWM`.
    #[expect(
        clippy::unused_self,
        reason = "the registry calls every handler as a method"
    )]
    fn report_memory(&self, args: &[String]) -> StepResult {
        let status =
            std::fs::read_to_string("/proc/self/status").map_err(|error| error.to_string())?;
        let field = |name: &str| {
            status
                .lines()
                .find_map(|line| line.strip_prefix(name))
                .map_or("?", str::trim)
                .to_owned()
        };
        println!(
            "  # memory {}: VmRSS {}, VmHWM {}",
            args[0],
            field("VmRSS:"),
            field("VmHWM:")
        );
        Ok(())
    }

    /// `report-load`: prints the current tab's load timings.
    fn report_load(&self, _: &[String]) -> StepResult {
        let stats = self.page()?.load_stats();
        let since = |at: Option<Instant>| {
            stats.started.zip(at).map_or_else(
                || "-".to_owned(),
                |(started, at)| format!("{:.1} ms", (at - started).as_secs_f64() * 1e3),
            )
        };
        println!(
            "  # load: decoded {}, first piece in {}, first frame {}, all in {}; {} \
             pieces, {} bytes, longest insert {:.1} ms",
            since(stats.decoded),
            since(stats.first_insert),
            since(stats.first_frame),
            since(stats.finished),
            stats.pieces,
            stats.bytes,
            stats.longest_insert.as_secs_f64() * 1e3
        );
        Ok(())
    }

    /// `assert-first-screen <release ms> [<debug ms>]`: the current tab's first frame came
    /// within the limit after its load started; a debug build takes the second limit, when
    /// given.
    fn assert_first_screen(&self, args: &[String]) -> StepResult {
        let stats = self.page()?.load_stats();
        let screen = stats
            .started
            .zip(stats.first_frame)
            .map(|(started, frame)| frame - started)
            .ok_or("no first frame was recorded")?;
        let debug = cfg!(debug_assertions) && args.len() > 1;
        let limit = Duration::from_millis(number(&args[usize::from(debug)])? as u64);
        println!(
            "  # first screen: {:.1} ms ({} build, limit {limit:?})",
            screen.as_secs_f64() * 1e3,
            build()
        );
        if screen <= limit {
            Ok(())
        } else {
            Err(format!("the first screen took {screen:?}, over {limit:?}"))
        }
    }

    /// `type-burst <count> <interval ms> <limit ms>`: types `count` characters `interval`
    /// apart the way GtkTextView commits a key (spike S1) and measures each from the insert
    /// to the end of the next painted frame; fails when one took longer than `limit`. Needs
    /// `unpace-frames`, or Broadway paints once a second.
    async fn type_burst(&self, args: &[String]) -> StepResult {
        let count = number(&args[0])?;
        let interval = Duration::from_millis(number(&args[1])? as u64);
        let limit = Duration::from_millis(number(&args[2])? as u64);
        let page = self.page()?;
        let view = page.view().clone();
        let buffer = page.buffer();
        let clock = self
            .window
            .window()
            .frame_clock()
            .ok_or("the window has no frame clock")?;
        let painted: Rc<RefCell<Vec<Instant>>> = Rc::new(RefCell::new(Vec::new()));
        let handler = clock.connect_after_paint(glib::clone!(
            #[strong]
            painted,
            move |_| painted.borrow_mut().push(Instant::now())
        ));
        let mut latencies = Vec::with_capacity(count);
        let mut result = Ok(());
        for _ in 0..count {
            glib::timeout_future(interval).await;
            let typed = Instant::now();
            buffer.begin_user_action();
            buffer.insert_interactive_at_cursor("x", view.is_editable());
            buffer.end_user_action();
            view.scroll_mark_onscreen(&buffer.get_insert());
            let frame = loop {
                if let Some(frame) = painted.borrow().iter().find(|frame| **frame > typed) {
                    break Some(*frame);
                }
                if typed.elapsed() > WAIT_TIMEOUT {
                    break None;
                }
                glib::timeout_future(Duration::from_millis(1)).await;
            };
            match frame {
                Some(frame) => latencies.push(frame - typed),
                None => {
                    result = Err("a keystroke was never painted".to_owned());
                    break;
                }
            }
        }
        clock.disconnect(handler);
        latencies.sort();
        let ms = |duration: Duration| duration.as_secs_f64() * 1e3;
        if let (Some(median), Some(longest)) =
            (latencies.get(latencies.len() / 2), latencies.last())
        {
            let p95 = latencies[(latencies.len() * 95).div_ceil(100).saturating_sub(1)];
            println!(
                "  # {} keystrokes, insert to painted frame: median {:.1} ms, p95 {:.1} ms, \
                 max {:.1} ms",
                latencies.len(),
                ms(*median),
                ms(p95),
                ms(*longest)
            );
            if *longest > limit {
                result = Err(format!(
                    "a keystroke took {longest:?} to paint, over {limit:?}"
                ));
            }
        }
        result
    }
}
