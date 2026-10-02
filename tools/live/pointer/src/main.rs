//! Injects pointer input through `zwlr_virtual_pointer_v1`, for live checks that need a mouse:
//! `stet-live-pointer [--extent W H] move X Y | click X Y | double-click X Y | right-click X Y |
//! back X Y | forward X Y | drag X1 Y1 X2 Y2 [STEPS] [DELAY_MS]`.
//! Coordinates are logical pixels of the output layout, as `hyprctl clients -j` reports them.

use std::thread::sleep;
use std::time::{Duration, Instant};
use wayland_client::protocol::{wl_pointer::ButtonState, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_SIDE: u32 = 0x113;
const BTN_EXTRA: u32 = 0x114;

#[derive(Default)]
struct Globals {
    seat: Option<wl_seat::WlSeat>,
    manager: Option<ZwlrVirtualPointerManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Globals {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(7), qh, ()));
                }
                "zwlr_virtual_pointer_manager_v1" => {
                    state.manager = Some(registry.bind(name, version.min(2), qh, ()));
                }
                _ => {}
            }
        }
    }
}

wayland_client::delegate_noop!(Globals: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(Globals: ignore ZwlrVirtualPointerManagerV1);
wayland_client::delegate_noop!(Globals: ignore ZwlrVirtualPointerV1);

struct Pointer {
    pointer: ZwlrVirtualPointerV1,
    queue: EventQueue<Globals>,
    globals: Globals,
    extent: (u32, u32),
    started: Instant,
}

impl Pointer {
    fn connect(extent: (u32, u32)) -> Result<Self, String> {
        let connection = Connection::connect_to_env().map_err(|e| e.to_string())?;
        let mut queue = connection.new_event_queue();
        let qh = queue.handle();
        connection.display().get_registry(&qh, ());
        let mut globals = Globals::default();
        queue.roundtrip(&mut globals).map_err(|e| e.to_string())?;
        let manager = globals
            .manager
            .clone()
            .ok_or("the compositor has no zwlr_virtual_pointer_manager_v1")?;
        let pointer = manager.create_virtual_pointer(globals.seat.as_ref(), &qh, ());
        queue.roundtrip(&mut globals).map_err(|e| e.to_string())?;
        Ok(Self {
            pointer,
            queue,
            globals,
            extent,
            started: Instant::now(),
        })
    }

    fn time(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    fn sync(&mut self) -> Result<(), String> {
        self.pointer.frame();
        self.queue
            .roundtrip(&mut self.globals)
            .map(drop)
            .map_err(|e| e.to_string())
    }

    fn move_to(&mut self, x: f64, y: f64) -> Result<(), String> {
        let (width, height) = self.extent;
        let x = x.clamp(0.0, f64::from(width - 1)).round() as u32;
        let y = y.clamp(0.0, f64::from(height - 1)).round() as u32;
        self.pointer
            .motion_absolute(self.time(), x, y, width, height);
        self.sync()
    }

    fn button(&mut self, state: ButtonState) -> Result<(), String> {
        self.button_of(BTN_LEFT, state)
    }

    fn button_of(&mut self, button: u32, state: ButtonState) -> Result<(), String> {
        self.pointer.button(self.time(), button, state);
        self.sync()
    }
}

fn number(args: &[String], index: usize) -> Result<f64, String> {
    args.get(index)
        .ok_or_else(|| format!("missing argument {index}"))?
        .parse()
        .map_err(|_| format!("argument {index} is not a number"))
}

fn run(mut args: Vec<String>) -> Result<(), String> {
    let mut extent = (3440, 1440);
    if args.first().is_some_and(|a| a == "--extent") {
        extent = (number(&args, 1)? as u32, number(&args, 2)? as u32);
        args.drain(..3);
    }
    let mut pointer = Pointer::connect(extent)?;
    let pause = |ms: u64| sleep(Duration::from_millis(ms));
    match args.first().map(String::as_str) {
        Some("move") => pointer.move_to(number(&args, 1)?, number(&args, 2)?),
        Some("click") => {
            pointer.move_to(number(&args, 1)?, number(&args, 2)?)?;
            pause(60);
            pointer.button(ButtonState::Pressed)?;
            pause(60);
            pointer.button(ButtonState::Released)
        }
        // Two clicks well inside GTK's 400 ms double-click time.
        Some("double-click") => {
            pointer.move_to(number(&args, 1)?, number(&args, 2)?)?;
            pause(60);
            for gap in [80, 0] {
                pointer.button(ButtonState::Pressed)?;
                pause(40);
                pointer.button(ButtonState::Released)?;
                pause(gap);
            }
            Ok(())
        }
        Some(name @ ("right-click" | "back" | "forward")) => {
            let button = match name {
                "right-click" => BTN_RIGHT,
                "back" => BTN_SIDE,
                _ => BTN_EXTRA,
            };
            pointer.move_to(number(&args, 1)?, number(&args, 2)?)?;
            pause(60);
            pointer.button_of(button, ButtonState::Pressed)?;
            pause(60);
            pointer.button_of(button, ButtonState::Released)
        }
        Some("drag") => {
            let (x1, y1, x2, y2) = (
                number(&args, 1)?,
                number(&args, 2)?,
                number(&args, 3)?,
                number(&args, 4)?,
            );
            let steps = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(40u32);
            let delay = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(15u64);
            pointer.move_to(x1, y1)?;
            pause(150);
            pointer.button(ButtonState::Pressed)?;
            pause(150);
            for step in 1..=steps {
                let t = f64::from(step) / f64::from(steps);
                pointer.move_to(x1 + (x2 - x1) * t, y1 + (y2 - y1) * t)?;
                pause(delay);
            }
            pause(300);
            pointer.button(ButtonState::Released)
        }
        _ => Err("usage: stet-live-pointer [--extent W H] move X Y | click X Y | double-click X Y | right-click X Y | back X Y | forward X Y | drag X1 Y1 X2 Y2 [STEPS] [DELAY_MS]".into()),
    }
}

fn main() {
    if let Err(error) = run(std::env::args().skip(1).collect()) {
        eprintln!("stet-live-pointer: {error}");
        std::process::exit(1);
    }
}
