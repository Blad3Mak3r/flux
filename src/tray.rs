use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(target_os = "linux")]
use std::thread;

use anyhow::{Context, Result};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::RuntimeStatus;

const REFRESH_INTERVAL: Duration = Duration::from_millis(250);

pub fn run(status: Arc<Mutex<RuntimeStatus>>, shutdown: Arc<AtomicBool>) -> Result<()> {
    #[cfg(windows)]
    return run_windows(status, shutdown);

    #[cfg(target_os = "linux")]
    return run_linux(status, shutdown);
}

#[cfg(target_os = "linux")]
fn run_linux(status: Arc<Mutex<RuntimeStatus>>, shutdown: Arc<AtomicBool>) -> Result<()> {
    let tray = Tray::new()?;
    while !shutdown.load(Ordering::Relaxed) {
        tray.refresh(&status);
        if clicked_quit(&tray.quit) {
            shutdown.store(true, Ordering::Relaxed);
            break;
        }
        thread::sleep(REFRESH_INTERVAL);
    }
    Ok(())
}

#[cfg(windows)]
fn run_windows(status: Arc<Mutex<RuntimeStatus>>, shutdown: Arc<AtomicBool>) -> Result<()> {
    use tao::event::Event;
    use tao::event_loop::{ControlFlow, EventLoop};

    let event_loop = EventLoop::<()>::new();
    let tray = Tray::new()?;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(std::time::Instant::now() + REFRESH_INTERVAL);
        if let Event::MainEventsCleared = event {
            tray.refresh(&status);
            if clicked_quit(&tray.quit) {
                shutdown.store(true, Ordering::Relaxed);
            }
            if shutdown.load(Ordering::Relaxed) {
                *control_flow = ControlFlow::Exit;
            }
        }
    });
}

struct Tray {
    _icon: TrayIcon,
    artnet: MenuItem,
    dmx: MenuItem,
    quit: MenuItem,
}

impl Tray {
    fn new() -> Result<Self> {
        let menu = Menu::new();
        let artnet = MenuItem::with_id("artnet-status", "Art-Net: starting", false, None);
        let dmx = MenuItem::with_id("dmx-status", "DMX: starting", false, None);
        let quit = MenuItem::with_id("quit", "Quit Flux", true, None);
        menu.append(&MenuItem::new(
            format!("Flux {}", env!("CARGO_PKG_VERSION")),
            false,
            None,
        ))?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&artnet)?;
        menu.append(&dmx)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit)?;

        let icon = Icon::from_rgba(icon_pixels(), 32, 32).context("Unable to create tray icon")?;
        let icon = TrayIconBuilder::new()
            .with_tooltip("Flux lighting bridge")
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .build()
            .context("Unable to create the Flux system tray icon")?;

        Ok(Self {
            _icon: icon,
            artnet,
            dmx,
            quit,
        })
    }

    fn refresh(&self, status: &Arc<Mutex<RuntimeStatus>>) {
        let status = status.lock().expect("runtime status mutex poisoned");
        self.artnet.set_text(format!("Art-Net: {}", status.artnet));
        self.dmx.set_text(format!("DMX: {}", status.dmx));
    }
}

fn clicked_quit(quit: &MenuItem) -> bool {
    MenuEvent::receiver()
        .try_iter()
        .any(|event| event.id == *quit.id())
}

fn icon_pixels() -> Vec<u8> {
    let mut pixels = vec![0; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let index = (y * 32 + x) * 4;
            let inside = (x as i32 - 16).pow(2) + (y as i32 - 16).pow(2) <= 14_i32.pow(2);
            if inside {
                pixels[index..index + 4].copy_from_slice(&[32, 118, 255, 255]);
            }
        }
    }
    for (x, y) in [(18, 5), (11, 17), (16, 17), (13, 27), (22, 13), (17, 13)] {
        let index = (y * 32 + x) * 4;
        pixels[index..index + 4].copy_from_slice(&[255, 255, 255, 255]);
    }
    pixels
}
