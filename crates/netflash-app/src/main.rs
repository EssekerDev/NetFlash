//! NetFlash tray host. The engine is the product; this file is plumbing.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![warn(missing_docs)]

#[cfg(target_os = "macos")]
mod macos;
mod prefs;
mod probes;
mod update;

use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use netflash_core::{is_wan_success, Band, Engine, EngineConfig, ProbeSample, Srgb8};
use netflash_icon::{paused_color, IconRenderer, Skin, DEFAULT_SIZE};
use probes::Prober;
use tokio::runtime::Runtime;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowId;

enum UserEvent {
    Wake,
    Menu(MenuEvent),
    UpdateReady { version: String, zip_url: String },
    UpdateFailed,
    QuitForUpdate,
}

enum ProbeCmd {
    Fire { count: u8 },
    Shutdown,
}

struct TraySwap {
    tray: TrayIcon,
    status: Menu,
    style: Menu,
}

thread_local! {
    static TRAY_SWAP: RefCell<Option<TraySwap>> = const { RefCell::new(None) };
}

struct App {
    start: Instant,
    engine: Engine,
    tray: Option<TrayIcon>,
    status_item: Option<MenuItem>,
    pause_item: Option<CheckMenuItem>,
    update_item: Option<MenuItem>,
    quit_item: Option<MenuItem>,
    last_icon: Option<(Skin, u8, u8, u8, u8)>,
    probe_tx: Sender<ProbeCmd>,
    probe_rx: Receiver<Vec<ProbeSample>>,
    probe_busy: bool,
    probe_started_ms: u64,
    paused: bool,
    skin: Skin,
    style_items: Vec<(Skin, CheckMenuItem)>,
    pending_zip: Option<String>,
    update_busy: bool,
    ui_proxy: EventLoopProxy<UserEvent>,
    last_logged_band: Option<Band>,
}

impl App {
    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn paint(&mut self) {
        let snap = self.engine.snapshot();
        let (color, score) = if self.paused {
            (paused_color(), 0.35)
        } else {
            (snap.displayed_color, snap.displayed_score)
        };
        let key = icon_key(self.skin, color, score);
        let tip = snap.tooltip();
        if let Some(status) = self.status_item.as_ref() {
            status.set_text(&tip);
        }
        if let Some(tray) = self.tray.as_ref() {
            let _ = tray.set_tooltip(Some(&tip));
        }
        if self.last_icon == Some(key) {
            return;
        }
        let icon = self.skin.render(color, score, DEFAULT_SIZE);
        let Ok(ti) = Icon::from_rgba(icon.rgba, icon.width, icon.height) else {
            return;
        };
        if let Some(tray) = self.tray.as_ref() {
            if tray.set_icon_with_as_template(Some(ti), false).is_ok() {
                #[cfg(target_os = "macos")]
                macos::force_color_image(tray);
                self.last_icon = Some(key);
            }
        }
        if self.last_logged_band != Some(snap.displayed_band) {
            self.last_logged_band = Some(snap.displayed_band);
            debug_log(&format!(
                "paint displayed={:.2} {:?} truth={:.2} {} {}",
                snap.displayed_score,
                snap.displayed_band,
                snap.quality.score,
                snap.displayed_color.to_hex(),
                tip
            ));
        }
    }

    fn ingest_live(&mut self, mut samples: Vec<ProbeSample>) {
        self.probe_busy = false;
        if samples.is_empty() {
            return;
        }
        // Stamp with host-now: a 70 ms success must not look 600 ms stale just
        // because a sibling probe was still timing out when the event arrived.
        let now = self.now_ms();
        let any_ok = samples.iter().any(is_wan_success);
        for s in &mut samples {
            s.at_ms = now;
        }
        self.engine.ingest_round(samples);
        let snap = self.engine.snapshot();
        debug_log(&format!(
            "ingest ok={any_ok} displayed={:.2} {:?} truth={:.2} wan={}",
            snap.displayed_score,
            snap.displayed_band,
            snap.quality.score,
            snap.quality.wan_reachable
        ));
    }

    fn drain_probe_results(&mut self) {
        while let Ok(samples) = self.probe_rx.try_recv() {
            self.ingest_live(samples);
        }
        if self.probe_busy && self.now_ms().saturating_sub(self.probe_started_ms) > 2_000 {
            self.probe_busy = false;
        }
    }

    fn maybe_probe(&mut self) {
        if self.paused || self.probe_busy {
            return;
        }
        if !self.engine.should_probe() {
            return;
        }
        let n = self.engine.scheduler().in_flight(self.engine.config());
        self.engine.mark_probed();
        self.probe_busy = true;
        self.probe_started_ms = self.now_ms();
        if self.probe_tx.send(ProbeCmd::Fire { count: n }).is_err() {
            self.probe_busy = false;
        }
    }

    fn build_tray(&mut self) {
        let status_menu = Menu::new();
        let status = MenuItem::new("NetFlash · starting…", false, None);
        let pause = CheckMenuItem::new("Pause", true, false, None);
        let update = MenuItem::new(
            format!("Version {}", env!("CARGO_PKG_VERSION")),
            false,
            None,
        );
        let quit = MenuItem::new("Quit", true, None);
        let _ = status_menu.append(&status);
        let _ = status_menu.append(&PredefinedMenuItem::separator());
        let _ = status_menu.append(&pause);
        let _ = status_menu.append(&update);
        let _ = status_menu.append(&quit);

        let style_menu = Menu::new();
        let appearance = MenuItem::new("Appearance", false, None);
        let _ = style_menu.append(&appearance);
        let _ = style_menu.append(&PredefinedMenuItem::separator());
        let mut style_items = Vec::new();
        for skin in Skin::ALL {
            let item = CheckMenuItem::new(skin.menu_label(), true, self.skin == skin, None);
            let _ = style_menu.append(&item);
            style_items.push((skin, item));
        }

        let color = self.engine.snapshot().displayed_color;
        let raster = self.skin.render(color, 0.0, DEFAULT_SIZE);
        let icon = match Icon::from_rgba(raster.rgba, raster.width, raster.height) {
            Ok(icon) => icon,
            Err(e) => {
                tracing::error!("icon: {e}");
                return;
            }
        };

        let tray = match TrayIconBuilder::new()
            .with_tooltip("NetFlash")
            .with_icon(icon)
            .with_menu(Box::new(status_menu.clone()))
            .with_icon_as_template(false)
            .with_menu_on_left_click(true)
            .build()
        {
            Ok(tray) => tray,
            Err(e) => {
                tracing::error!("tray: {e}");
                return;
            }
        };

        TRAY_SWAP.with(|slot| {
            *slot.borrow_mut() = Some(TraySwap {
                tray: tray.clone(),
                status: status_menu,
                style: style_menu,
            });
        });

        self.status_item = Some(status);
        self.pause_item = Some(pause);
        self.update_item = Some(update);
        self.quit_item = Some(quit);
        self.style_items = style_items;
        self.tray = Some(tray);
        self.last_icon = Some(icon_key(self.skin, color, 0.0));
        #[cfg(target_os = "macos")]
        if let Some(tray) = self.tray.as_ref() {
            macos::force_color_image(tray);
        }
    }

    fn on_menu(&mut self, event_loop: &ActiveEventLoop, event: MenuEvent) {
        if let Some(quit) = self.quit_item.as_ref() {
            if event.id() == quit.id() {
                let _ = self.probe_tx.send(ProbeCmd::Shutdown);
                event_loop.exit();
                return;
            }
        }
        if let Some(pause) = self.pause_item.as_ref() {
            if event.id() == pause.id() {
                self.paused = !self.paused;
                self.engine.set_paused(self.paused);
                pause.set_checked(self.paused);
                self.last_icon = None;
                self.paint();
                return;
            }
        }
        if let Some(update) = self.update_item.as_ref() {
            if event.id() == update.id() {
                self.start_update();
                return;
            }
        }
        for (skin, item) in &self.style_items {
            if event.id() == item.id() {
                self.set_skin(*skin);
                return;
            }
        }
    }

    fn set_skin(&mut self, skin: Skin) {
        self.skin = skin;
        prefs::save_skin(skin);
        for (s, item) in &self.style_items {
            item.set_checked(*s == skin);
        }
        self.last_icon = None;
        self.paint();
    }

    fn on_update_ready(&mut self, version: String, zip_url: String) {
        tracing::info!("update available: {version}");
        self.pending_zip = Some(zip_url);
        if let Some(item) = self.update_item.as_ref() {
            item.set_text("Update");
            item.set_enabled(true);
        }
    }

    fn start_update(&mut self) {
        if self.update_busy {
            return;
        }
        let Some(url) = self.pending_zip.clone() else {
            return;
        };
        let Some(dest) = update::current_app_bundle() else {
            return;
        };
        self.update_busy = true;
        if let Some(item) = self.update_item.as_ref() {
            item.set_enabled(false);
            item.set_text("Updating…");
        }
        let proxy = self.ui_proxy.clone();
        thread::Builder::new()
            .name("netflash-update".into())
            .spawn(move || match update::stage_replace(&url, &dest) {
                Ok(()) => {
                    let _ = proxy.send_event(UserEvent::QuitForUpdate);
                }
                Err(e) => {
                    tracing::warn!("update: {e}");
                    let _ = proxy.send_event(UserEvent::UpdateFailed);
                }
            })
            .ok();
    }

    fn on_update_failed(&mut self) {
        self.update_busy = false;
        self.pending_zip = None;
        if let Some(item) = self.update_item.as_ref() {
            item.set_text(format!("Version {}", env!("CARGO_PKG_VERSION")));
            item.set_enabled(false);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if cause == StartCause::Init {
            self.build_tray();
        }
        self.tick(event_loop);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let _ = event_loop;
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Wake => {}
            UserEvent::Menu(event) => self.on_menu(event_loop, event),
            UserEvent::UpdateReady { version, zip_url } => self.on_update_ready(version, zip_url),
            UserEvent::UpdateFailed => self.on_update_failed(),
            UserEvent::QuitForUpdate => {
                let _ = self.probe_tx.send(ProbeCmd::Shutdown);
                event_loop.exit();
            }
        }
        self.tick(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if event == WindowEvent::CloseRequested {
            event_loop.exit();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.tick(event_loop);
    }
}

impl App {
    fn tick(&mut self, event_loop: &ActiveEventLoop) {
        self.drain_probe_results();
        // Freeze the engine clock while a round is in flight. Otherwise the
        // 800 ms dead-man fires during a 600 ms timeout and recovery never
        // leaves violet — CLI does not have this bug because it blocks.
        if !self.probe_busy {
            self.engine.advance_to(self.now_ms());
        }
        self.maybe_probe();
        self.paint();
        schedule_wakeup(event_loop, self);
    }
}

fn schedule_wakeup(event_loop: &ActiveEventLoop, app: &App) {
    let now = Instant::now();
    let snap = app.engine.snapshot();
    let animating = !app.paused
        && ((snap.displayed_score - snap.quality.score).abs() > 0.004
            || (snap.displayed_score > 0.002 && !snap.quality.wan_reachable));
    let frame = if animating {
        now + Duration::from_millis(33)
    } else {
        now + Duration::from_millis(250)
    };
    event_loop.set_control_flow(ControlFlow::WaitUntil(frame));
}

fn spawn_prober(
    start: Instant,
    tx_events: EventLoopProxy<UserEvent>,
) -> (Sender<ProbeCmd>, Receiver<Vec<ProbeSample>>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    thread::Builder::new()
        .name("netflash-probes".into())
        .spawn(move || {
            let rt = match Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("tokio runtime: {e}");
                    return;
                }
            };
            let prober = {
                let _enter = rt.enter();
                match Prober::new(start) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("prober: {e}");
                        return;
                    }
                }
            };
            while let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    ProbeCmd::Shutdown => break,
                    ProbeCmd::Fire { count } => {
                        let samples = rt.block_on(prober.round(count));
                        let _ = result_tx.send(samples);
                        let _ = tx_events.send_event(UserEvent::Wake);
                    }
                }
            }
        })
        .expect("probe thread");
    (cmd_tx, result_rx)
}

fn spawn_update_check(proxy: EventLoopProxy<UserEvent>) {
    if update::current_app_bundle().is_none() {
        return;
    }
    thread::Builder::new()
        .name("netflash-update-check".into())
        .spawn(
            move || match update::fetch_latest(env!("CARGO_PKG_VERSION")) {
                Ok(Some(latest)) => {
                    let _ = proxy.send_event(UserEvent::UpdateReady {
                        version: latest.version,
                        zip_url: latest.zip_url,
                    });
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("update check: {e}"),
            },
        )
        .ok();
}

fn run_cli(start: Instant, cfg: EngineConfig) {
    let rt = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("tokio: {e}");
            return;
        }
    };
    let prober = {
        let _enter = rt.enter();
        match Prober::new(start) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("prober: {e}");
                return;
            }
        }
    };
    let mut engine = Engine::new(cfg);
    let deadline = start + Duration::from_secs(8);
    println!(
        "NetFlash --cli (8 s) — the tray stays violet briefly on boot (conservative recovery)."
    );
    while Instant::now() < deadline {
        let now = start.elapsed().as_millis() as u64;
        engine.advance_to(now);
        if engine.should_probe() {
            let n = engine.scheduler().in_flight(engine.config());
            engine.mark_probed();
            let samples = rt.block_on(prober.round(n));
            engine.ingest_round(samples);
        }
        let snap = engine.snapshot();
        println!(
            "{:>5} ms  paint={:.2} {:?}  truth={:.2}  {}  {}",
            snap.now_ms,
            snap.displayed_score,
            snap.displayed_band,
            snap.quality.score,
            snap.displayed_color.to_hex(),
            snap.tooltip()
        );
        thread::sleep(Duration::from_millis(250));
    }
}

fn run_sim(start: Instant, cfg: EngineConfig) {
    let (probe_tx, probe_rx) = {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        thread::spawn(move || {
            let rt = Runtime::new().expect("tokio");
            let _enter = rt.enter();
            let prober = Prober::new(start).expect("prober");
            drop(_enter);
            while let Ok(cmd) = cmd_rx.recv() {
                match cmd {
                    ProbeCmd::Shutdown => break,
                    ProbeCmd::Fire { count } => {
                        let samples = rt.block_on(prober.round(count));
                        let _ = result_tx.send(samples);
                    }
                }
            }
        });
        (cmd_tx, result_rx)
    };
    let mut engine = Engine::new(cfg);
    let mut busy = false;
    println!("NetFlash --sim (5 s, same loop as the tray, no icon)");
    let deadline = start + Duration::from_secs(5);
    while Instant::now() < deadline {
        while let Ok(mut samples) = probe_rx.try_recv() {
            busy = false;
            let now = start.elapsed().as_millis() as u64;
            for s in &mut samples {
                s.at_ms = now;
            }
            engine.ingest_round(samples);
        }
        if !busy {
            engine.advance_to(start.elapsed().as_millis() as u64);
        }
        if !busy && engine.should_probe() {
            let n = engine.scheduler().in_flight(engine.config());
            engine.mark_probed();
            busy = true;
            let _ = probe_tx.send(ProbeCmd::Fire { count: n });
        }
        let snap = engine.snapshot();
        println!(
            "{:>5} ms  paint={:.2} {:?}  truth={:.2} wan={}  {}",
            snap.now_ms,
            snap.displayed_score,
            snap.displayed_band,
            snap.quality.score,
            snap.quality.wan_reachable,
            snap.tooltip()
        );
        thread::sleep(Duration::from_millis(50));
    }
    let _ = probe_tx.send(ProbeCmd::Shutdown);
}

fn icon_key(skin: Skin, color: Srgb8, score: f64) -> (Skin, u8, u8, u8, u8) {
    (
        skin,
        color.r,
        color.g,
        color.b,
        (score.clamp(0.0, 1.0) * 50.0).round() as u8,
    )
}

fn debug_log(line: &str) {
    if std::env::var_os("NETFLASH_DEBUG").is_none() {
        return;
    }
    let path = std::env::temp_dir().join("netflash.log");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let start = Instant::now();
    let mut cfg = EngineConfig::default();
    if std::env::var_os("NETFLASH_REDUCED_MOTION").is_some() {
        cfg.reduced_motion = true;
    }

    if std::env::args().any(|a| a == "--cli") {
        run_cli(start, cfg);
        return;
    }
    if std::env::args().any(|a| a == "--sim") {
        run_sim(start, cfg);
        return;
    }

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = builder.build().expect("event loop");
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |e| {
            let _ = proxy.send_event(UserEvent::Menu(e));
        }
    }));
    TrayIconEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |event| {
            if let TrayIconEvent::Click {
                button,
                button_state: MouseButtonState::Down,
                ..
            } = event
            {
                TRAY_SWAP.with(|slot| {
                    let slot = slot.borrow();
                    let Some(swap) = slot.as_ref() else {
                        return;
                    };
                    match button {
                        MouseButton::Left => {
                            swap.tray.set_menu(Some(Box::new(swap.status.clone())));
                        }
                        MouseButton::Right => {
                            swap.tray.set_menu(Some(Box::new(swap.style.clone())));
                        }
                        _ => {}
                    }
                });
            }
            let _ = proxy.send_event(UserEvent::Wake);
        }
    }));
    let (probe_tx, probe_rx) = spawn_prober(start, proxy.clone());
    spawn_update_check(proxy.clone());

    tracing::info!("NetFlash: left-click status / Pause / Update / Quit; right-click appearance.");

    let mut app = App {
        start,
        engine: Engine::new(cfg),
        tray: None,
        status_item: None,
        pause_item: None,
        update_item: None,
        quit_item: None,
        last_icon: None,
        probe_tx,
        probe_rx,
        probe_busy: false,
        probe_started_ms: 0,
        paused: false,
        skin: prefs::load_skin(),
        style_items: Vec::new(),
        pending_zip: None,
        update_busy: false,
        ui_proxy: proxy,
        last_logged_band: None,
    };

    event_loop.run_app(&mut app).expect("run");
}
