//! M0 performance benchmark for the LexTrail desktop app (`docs/ARCHITECTURE.md` §10).
//!
//! Drives a release build (`pnpm --filter @lextrail/desktop tauri build --no-bundle`)
//! with simulated hotkeys and measures memory, CPU and popup show times. It moves the
//! mouse cursor and sends key presses, so leave the machine alone while it runs.
//!
//! ```text
//! lextrail-bench quick [--app PATH] [--cold N] [--samples N] [--lookup-text TEXT]
//! lextrail-bench long  [--app PATH] [--wait-mins N] [--lookup-text TEXT]
//! ```
//!
//! The hotkey is pressed over the benchmark's own terminal, where nothing is selected, so
//! the popup shows "no selection" and the dictionary stays closed. `--lookup-text launch`
//! makes the app look that text up instead (`LEXTRAIL_BENCH_LOOKUP_TEXT`): the dictionary
//! is opened by the first lookup and a dictionary result is on screen.

mod procs;
mod stats;
mod timings;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    run::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("lextrail-bench only runs on Windows");
    std::process::exit(2);
}

#[cfg(windows)]
mod run {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, bail};
    use lextrail_config::{PopupMode, Settings};

    use crate::procs::{self, Memory, TreeSample, mib};
    use crate::stats::{Summary, cpu_percent, ms};
    use crate::timings::{self, Timing};
    use crate::ui;

    const APP_NAME: &str = "lextrail-desktop.exe";
    /// Read by the app only when [`timings::ENV`] is set too.
    const LOOKUP_TEXT_ENV: &str = "LEXTRAIL_BENCH_LOOKUP_TEXT";
    const DEFAULT_APP: &str = r"target\release\lextrail-desktop.exe";
    /// Time for the tray app to finish starting before it is measured.
    const STARTUP_SETTLE: Duration = Duration::from_secs(4);
    /// Comfortably past the app's 5 s SUSPEND_DELAY.
    const SUSPEND_SETTLE: Duration = Duration::from_secs(15);
    const SHOW_TIMEOUT: Duration = Duration::from_secs(10);
    const IDLE_WINDOW: Duration = Duration::from_secs(60);
    const HUMAN_REACTION: Duration = Duration::from_millis(150);

    struct Options {
        command: String,
        app: PathBuf,
        cold: usize,
        samples: usize,
        wait_mins: u64,
        lookup_text: Option<String>,
    }

    fn parse_args() -> Result<Options> {
        let mut args = std::env::args().skip(1);
        let command = args.next().unwrap_or_else(|| "quick".into());
        let mut options = Options {
            command,
            app: PathBuf::from(DEFAULT_APP),
            cold: 5,
            samples: 10,
            wait_mins: 120,
            lookup_text: None,
        };
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .with_context(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--app" => options.app = value.into(),
                "--cold" => options.cold = value.parse()?,
                "--samples" => options.samples = value.parse()?,
                "--wait-mins" => options.wait_mins = value.parse()?,
                "--lookup-text" => options.lookup_text = Some(value),
                other => bail!("unknown option {other}"),
            }
        }
        Ok(options)
    }

    pub fn main() -> Result<()> {
        let options = parse_args()?;
        check_default_hotkey()?;
        if !options.app.exists() {
            bail!(
                "{} not found; build it with `pnpm --filter @lextrail/desktop tauri build --no-bundle`",
                options.app.display()
            );
        }
        let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
        println!("app: {}", options.app.display());
        println!("logical CPUs: {cpus}");
        match &options.lookup_text {
            Some(text) => println!("lookup text: {text:?} (dictionary opened by the first lookup)"),
            None => println!("lookup text: none (no selection; dictionary not opened)"),
        }
        match options.command.as_str() {
            "quick" => quick(&options, cpus),
            "long" => long(&options),
            other => bail!("unknown command {other} (expected quick or long)"),
        }
    }

    /// The app runs with default settings in a scratch directory (it never reads the
    /// user's own while benchmarked); this checks that the default is what
    /// [`ui::press_hotkey`] presses, and the default (responsive) popup mode.
    fn check_default_hotkey() -> Result<()> {
        let settings = Settings::default();
        if settings.hotkeys.lookup != "Alt+Space" || settings.popup.mode != PopupMode::Responsive {
            bail!(
                "the default hotkey is now {} (mode {:?}): update ui::press_hotkey",
                settings.hotkeys.lookup,
                settings.popup.mode
            );
        }
        Ok(())
    }

    /// The app's settings directory while benchmarked (`paths.rs` in the app): emptied
    /// before each start, so the app writes the current defaults.
    fn scratch_config_dir() -> PathBuf {
        std::env::temp_dir().join("lextrail-bench-config")
    }

    fn quick(options: &Options, cpus: usize) -> Result<()> {
        // 1. Cold start: a fresh app each time, first sample discarded as warm-up.
        let mut cold = Vec::new();
        let mut cold_split = Vec::new();
        for i in 0..=options.cold {
            let app = App::launch(options)?;
            if i == 0 {
                section("tray only (no WebView)");
                report_tree("memory", &app.sample()?);
                report_cpu("idle CPU", &app, cpus)?;
            }
            let t = app.show()?;
            app.hide()?;
            let split = app.timing()?;
            if i > 0 {
                cold.push(t);
                cold_split.push(split);
            }
            app.quit()?;
        }
        section("cold start: hotkey -> popup visible (fresh app each time)");
        print_summary(&cold);
        print_split(&cold_split);

        // 2. One app for warm, visible and suspended measurements.
        let app = App::launch(options)?;
        app.show()?; // cold, not counted
        sleep(Duration::from_millis(500));
        section("popup visible");
        report_tree("memory", &app.sample()?);
        app.hide()?;
        app.timing()?;

        let mut warm = Vec::new();
        let mut warm_split = Vec::new();
        for _ in 0..options.samples {
            sleep(Duration::from_secs(1));
            warm.push(app.show()?);
            sleep(Duration::from_millis(300));
            app.hide()?;
            warm_split.push(app.timing()?);
        }
        section("warm show: hidden ~1 s, not yet suspended");
        print_summary(&warm);
        print_split(&warm_split);

        let mut resumed = Vec::new();
        let mut resumed_split = Vec::new();
        for i in 0..options.samples {
            sleep(SUSPEND_SETTLE);
            if i == 0 {
                section("suspended (hidden 15 s)");
                report_tree("memory", &app.sample()?);
                report_cpu("idle CPU", &app, cpus)?;
            }
            resumed.push(app.show()?);
            sleep(Duration::from_millis(300));
            app.hide()?;
            resumed_split.push(app.timing()?);
        }
        section("show after suspend (hidden 15 s)");
        print_summary(&resumed);
        print_split(&resumed_split);

        // 3. The settings window (D047) shares the WebView2 environment with the popup;
        // what stays after it is closed (docs/specs/m1-hotkey-settings.md 8.3).
        let settings = app.open_settings(options)?;
        sleep(Duration::from_secs(2));
        section("settings window open (popup suspended)");
        report_tree("memory", &app.sample()?);
        ui::close(settings)?;
        ui::wait_gone(settings, SHOW_TIMEOUT)?;
        app.show()?;
        sleep(Duration::from_millis(300));
        app.hide()?;
        app.timing()?;
        sleep(SUSPEND_SETTLE);
        section("suspended after the settings window was opened and closed (hidden 15 s)");
        report_tree("memory", &app.sample()?);
        report_cpu("idle CPU", &app, cpus)?;
        app.quit()
    }

    fn long(options: &Options) -> Result<()> {
        let app = App::launch(options)?;
        app.show()?;
        sleep(Duration::from_millis(500));
        app.hide()?;
        sleep(SUSPEND_SETTLE);
        section("suspended, start of long wait");
        report_tree("memory", &app.sample()?);

        let total = Duration::from_secs(options.wait_mins * 60);
        let started = Instant::now();
        let step = Duration::from_secs(30 * 60);
        while started.elapsed() < total {
            sleep(step.min(total.saturating_sub(started.elapsed())));
            let mins = started.elapsed().as_secs() / 60;
            report_tree(&format!("memory after {mins} min"), &app.sample()?);
        }
        section(&format!("show after {} min suspended", options.wait_mins));
        let t = app.show()?;
        println!("  {}", ms(t));
        sleep(Duration::from_millis(500));
        report_tree("memory after resume", &app.sample()?);
        app.hide()?;
        // Second show right after, for comparison with the long-suspended one.
        sleep(Duration::from_secs(1));
        println!("  next show (hidden 1 s): {}", ms(app.show()?));
        app.hide()?;
        app.quit()
    }

    struct App {
        child: Child,
        /// File the app appends one timing line per lookup to.
        timings: PathBuf,
        timings_read: Cell<usize>,
    }

    impl App {
        fn launch(options: &Options) -> Result<Self> {
            kill_all()?;
            let timings = std::env::temp_dir().join("lextrail-bench-timings.txt");
            let _ = std::fs::remove_file(&timings);
            let _ = std::fs::remove_dir_all(scratch_config_dir());
            let mut command = Command::new(&options.app);
            command.env(timings::ENV, &timings);
            if let Some(text) = &options.lookup_text {
                command.env(LOOKUP_TEXT_ENV, text);
            }
            let child = command
                .spawn()
                .with_context(|| format!("cannot start {}", options.app.display()))?;
            sleep(STARTUP_SETTLE);
            Ok(Self {
                child,
                timings,
                timings_read: Cell::new(0),
            })
        }

        /// The app's own timings for the next lookup not read yet. The app writes the
        /// line once the popup has focus, so call this after [`App::hide`].
        fn timing(&self) -> Result<Timing> {
            let start = Instant::now();
            loop {
                let text = std::fs::read_to_string(&self.timings).unwrap_or_default();
                if let Some(line) = text.lines().nth(self.timings_read.get()) {
                    self.timings_read.set(self.timings_read.get() + 1);
                    return Ok(timings::parse_line(line));
                }
                if start.elapsed() > SHOW_TIMEOUT {
                    bail!(
                        "no timing line in {} (is the app new enough?)",
                        self.timings.display()
                    );
                }
                sleep(Duration::from_millis(20));
            }
        }

        fn pid(&self) -> u32 {
            self.child.id()
        }

        /// Presses the hotkey and returns the time until the popup is visible.
        fn show(&self) -> Result<Duration> {
            ui::park_cursor()?;
            let start = Instant::now();
            ui::press_hotkey()?;
            ui::wait_popup(self.pid(), true, start, SHOW_TIMEOUT)
        }

        fn hide(&self) -> Result<()> {
            ui::wait_popup_focused(self.pid(), SHOW_TIMEOUT)?;
            // Right after a cold show the WebView takes keyboard focus a little later
            // than the window; an Esc within that gap is lost. No human is that fast.
            sleep(HUMAN_REACTION);
            let start = Instant::now();
            ui::press_escape()?;
            ui::wait_popup(self.pid(), false, start, SHOW_TIMEOUT)?;
            Ok(())
        }

        /// Opens the settings window the way a shortcut would (`--settings` to the running
        /// app) and waits until it is shown (the app shows it once the page has loaded).
        fn open_settings(&self, options: &Options) -> Result<windows::Win32::Foundation::HWND> {
            Command::new(&options.app)
                .arg("--settings")
                .status()
                .context("cannot start the app with --settings")?;
            let start = Instant::now();
            loop {
                if let Some(hwnd) = ui::find_settings(self.pid()).filter(|&h| ui::is_visible(h)) {
                    return Ok(hwnd);
                }
                if start.elapsed() > SHOW_TIMEOUT {
                    bail!("the settings window did not show within {SHOW_TIMEOUT:?}");
                }
                sleep(Duration::from_millis(20));
            }
        }

        fn sample(&self) -> Result<TreeSample> {
            procs::sample_tree(self.pid())
        }

        fn quit(mut self) -> Result<()> {
            let _ = self.child.kill();
            let _ = self.child.wait();
            kill_all()
        }
    }

    /// Stops every running instance and waits for its WebView2 children to exit.
    fn kill_all() -> Result<()> {
        let all = procs::list_processes()?;
        let roots: Vec<u32> = all
            .iter()
            .filter(|p| p.name.eq_ignore_ascii_case(APP_NAME))
            .map(|p| p.pid)
            .collect();
        let mut family = std::collections::BTreeSet::new();
        for &root in &roots {
            family.extend(procs::tree(&all, root));
            let _ = Command::new("taskkill")
                .args(["/F", "/PID", &root.to_string()])
                .output();
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let alive = procs::list_processes()?
                .iter()
                .any(|p| family.contains(&p.pid));
            if !alive {
                return Ok(());
            }
            sleep(Duration::from_millis(200));
        }
        bail!("old LexTrail processes did not exit")
    }

    fn section(title: &str) {
        println!("\n## {title}");
    }

    fn print_summary(samples: &[Duration]) {
        match Summary::of(samples) {
            Some(s) => println!("  {s}"),
            None => println!("  (no samples)"),
        }
    }

    /// The app's own split of each show: capture vs. window.
    fn print_split(timings: &[Timing]) {
        type Field = fn(&Timing) -> Option<Duration>;
        let rows: [(&str, Field); 4] = [
            ("capture (in app)", |t| t.capture),
            ("window ready (from hotkey)", |t| t.window_ready),
            ("window shown (from hotkey)", |t| t.shown),
            ("focused (from hotkey)", |t| t.focused),
        ];
        for (label, get) in rows {
            let samples: Vec<Duration> = timings.iter().filter_map(get).collect();
            match Summary::of(&samples) {
                Some(s) => println!("    {label:<27} {s}"),
                None => println!("    {label:<27} (no samples)"),
            }
        }
    }

    fn report_tree(label: &str, sample: &TreeSample) {
        let line = |m: Memory| {
            format!(
                "private WS {:6.1} MiB | WS {:6.1} MiB | private commit {:6.1} MiB",
                mib(m.private_working_set),
                mib(m.working_set),
                mib(m.private_commit)
            )
        };
        println!("  {label}: {} process(es)", sample.count());
        println!("    total                 {}", line(sample.total()));
        for (info, m) in &sample.processes {
            println!(
                "    {:>6} {:<15} {}",
                info.pid,
                truncate(&info.name, 15),
                line(*m)
            );
        }
    }

    fn report_cpu(label: &str, app: &App, cpus: usize) -> Result<()> {
        let before: BTreeMap<u32, Duration> = procs::tree_cpu(app.pid())?;
        let start = Instant::now();
        sleep(IDLE_WINDOW);
        let after = procs::tree_cpu(app.pid())?;
        let wall = start.elapsed();
        let used = procs::cpu_delta(&before, &after);
        println!(
            "  {label}: {} CPU over {:.0} s = {:.3} % of machine",
            ms(used),
            wall.as_secs_f64(),
            cpu_percent(used, wall, cpus)
        );
        Ok(())
    }

    fn truncate(s: &str, n: usize) -> String {
        s.chars().take(n).collect()
    }
}
