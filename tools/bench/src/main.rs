//! M0 performance benchmark for the LexTrail desktop app (`docs/ARCHITECTURE.md` §10).
//!
//! Drives a release build (`pnpm --filter @lextrail/desktop tauri build --no-bundle`)
//! with simulated hotkeys and measures memory, CPU and popup show times. It moves the
//! mouse cursor and sends key presses, so leave the machine alone while it runs.
//!
//! ```text
//! lextrail-bench quick [--app PATH] [--cold N] [--samples N]
//! lextrail-bench long  [--app PATH] [--wait-mins N]
//! ```

mod procs;
mod stats;
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
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use anyhow::{Context, Result, bail};
    use lextrail_config::{PopupMode, Settings, SettingsFile};

    use crate::procs::{self, Memory, TreeSample, mib};
    use crate::stats::{Summary, cpu_percent, ms};
    use crate::ui;

    const APP_NAME: &str = "lextrail-desktop.exe";
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
                other => bail!("unknown option {other}"),
            }
        }
        Ok(options)
    }

    pub fn main() -> Result<()> {
        let options = parse_args()?;
        check_settings()?;
        if !options.app.exists() {
            bail!(
                "{} not found; build it with `pnpm --filter @lextrail/desktop tauri build --no-bundle`",
                options.app.display()
            );
        }
        let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
        println!("app: {}", options.app.display());
        println!("logical CPUs: {cpus}");
        match options.command.as_str() {
            "quick" => quick(&options, cpus),
            "long" => long(&options),
            other => bail!("unknown command {other} (expected quick or long)"),
        }
    }

    /// The benchmark relies on the default hotkey and the default (responsive) mode.
    fn check_settings() -> Result<()> {
        let dir = std::env::var("APPDATA").context("APPDATA is not set")?;
        let file = SettingsFile::in_dir(PathBuf::from(dir).join("io.github.aspeternity.lextrail"));
        let settings = if file.path().exists() {
            file.load()?.into_settings()
        } else {
            Settings::default()
        };
        if settings.hotkeys.lookup != "Ctrl+Alt+X" || settings.popup.mode != PopupMode::Responsive {
            bail!(
                "{} must use the default hotkey (Ctrl+Alt+X) and popup mode (responsive)",
                file.path().display()
            );
        }
        Ok(())
    }

    fn quick(options: &Options, cpus: usize) -> Result<()> {
        // 1. Cold start: a fresh app each time, first sample discarded as warm-up.
        let mut cold = Vec::new();
        for i in 0..=options.cold {
            let app = App::launch(options)?;
            if i == 0 {
                section("tray only (no WebView)");
                report_tree("memory", &app.sample()?);
                report_cpu("idle CPU", &app, cpus)?;
            }
            let t = app.show()?;
            if i > 0 {
                cold.push(t);
            }
            app.hide()?;
            app.quit()?;
        }
        section("cold start: hotkey -> popup visible (fresh app each time)");
        print_summary(&cold);

        // 2. One app for warm, visible and suspended measurements.
        let app = App::launch(options)?;
        app.show()?; // cold, not counted
        sleep(Duration::from_millis(500));
        section("popup visible");
        report_tree("memory", &app.sample()?);
        app.hide()?;

        let mut warm = Vec::new();
        for _ in 0..options.samples {
            sleep(Duration::from_secs(1));
            warm.push(app.show()?);
            sleep(Duration::from_millis(300));
            app.hide()?;
        }
        section("warm show: hidden ~1 s, not yet suspended");
        print_summary(&warm);

        let mut resumed = Vec::new();
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
        }
        section("show after suspend (hidden 15 s)");
        print_summary(&resumed);
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
    }

    impl App {
        fn launch(options: &Options) -> Result<Self> {
            kill_all()?;
            let child = Command::new(&options.app)
                .spawn()
                .with_context(|| format!("cannot start {}", options.app.display()))?;
            sleep(STARTUP_SETTLE);
            Ok(Self { child })
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
