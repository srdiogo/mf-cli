// SPDX-License-Identifier: MIT
mod config;
mod legacy;
mod mixer;
mod model;
mod state;
mod transport;

use clap::{Parser, Subcommand, ValueEnum};
use config::Snapshot;
use state::{Change, Mix, State};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mf-cli",
    version,
    about = "Control Arturia MiniFuse hardware and the MiniFuse 4 mixer",
    after_help = "Legacy toggles remain available: mf-cli <target> <on|off> [<target> <on|off> ...]\nMiniFuse 1/2: inst, 48v, monitor. MiniFuse 4: inst/inst1, inst2, 48v.\nNew status/mixer/config commands require the updated kernel module; use config show for offline snapshots."
)]
struct Cli {
    /// Directory for durable backups and operation records before mixer changes
    #[arg(long, global = true)]
    backup_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Read MiniFuse 4 mixer configuration (not signal meters)
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Adjust one MiniFuse 4 internal mix
    Mixer {
        #[arg(value_enum)]
        mix: Mix,
        #[command(subcommand)]
        action: MixerAction,
    },
    /// Route out12 (usb12/mix), out34 (usb34/mix), or loopback (usb56/mix/out12/out34)
    Route {
        #[arg(value_enum)]
        mix: Mix,
        source: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Save, inspect, compare or restore host-side mixer configuration files
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Toggle {
    On,
    Off,
}
impl Toggle {
    fn enabled(self) -> bool {
        matches!(self, Self::On)
    }
}

#[derive(Subcommand)]
enum MixerAction {
    /// Set a source; pan is 0 (left)..1 (right), level is -60..12 dB or -inf
    Channel {
        source: String,
        #[arg(long, allow_hyphen_values = true)]
        level: Option<String>,
        #[arg(long)]
        pan: Option<f64>,
        #[arg(long, value_enum)]
        mute: Option<Toggle>,
        #[arg(long, value_enum)]
        solo: Option<Toggle>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Set master level (-64..0 dB or -inf)
    Master {
        #[arg(allow_hyphen_values = true)]
        level: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Include/remove an analog mono source or a USB stereo pair
    Source {
        source: String,
        #[arg(value_enum)]
        state: Toggle,
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Save a new snapshot; existing files are never overwritten
    Save { file: PathBuf },
    /// Inspect a snapshot offline, without opening USB or the kernel node
    Show {
        file: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Compare restorable fields with live state without writing USB
    Diff {
        file: PathBuf,
        #[arg(long)]
        against: Option<PathBuf>,
        #[arg(long, value_enum)]
        mix: Option<Mix>,
    },
    /// Restore captured writable mixer fields; opaque bytes and trims stay unchanged
    Restore {
        file: PathBuf,
        #[arg(long, value_enum)]
        mix: Option<Mix>,
        #[arg(long)]
        dry_run: bool,
    },
}

fn show(device: &transport::Device, state: &State, json: bool) -> Result<(), String> {
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "device": device, "state": state.decoded(), "payload_hex": state::hex(&state.0) })).map_err(|e| e.to_string())?);
    } else {
        println!(
            "MiniFuse 4 {:04x}:{:04x} (bus {}, address {})",
            device.vendor_id, device.product_id, device.bus, device.address
        );
        for mix in Mix::ALL {
            let i = mix.index();
            println!(
                "{}: route={}, master={} dB",
                mix.name(),
                mix.route_name(state.word(176 + i * 2)),
                state
                    .db(138 + i * 2)
                    .map(|x| format!("{x:.3}"))
                    .unwrap_or("-inf".into())
            );
            for (ch, name) in state::CHANNEL_NAMES.iter().enumerate().take(mix.channels()) {
                let value = |offset| {
                    state
                        .db(offset)
                        .map(|x| format!("{x:.3}"))
                        .unwrap_or("-inf".into())
                };
                println!(
                    "  {name:7} L={:>8} R={:>8} dB mute={} solo={}",
                    value(18 + i * 40 + ch * 2),
                    value(38 + i * 40 + ch * 2),
                    state.word(152 + i * 2) & (1 << ch) != 0,
                    state.word(158 + i * 2) & (1 << ch) != 0
                );
            }
        }
        println!(
            "Stored gains only; signal meters are not implemented. Input trims are available in --json."
        );
    }
    Ok(())
}

fn plan_json(state: &State, plan: &[Change], saved: Option<&State>) -> Result<(), String> {
    let predicted = state::predict(state, plan)?;
    let ignored: Vec<usize> = saved
        .map(|s| {
            (0..state::STATE_SIZE)
                .filter(|&i| s.0[i] != predicted.0[i])
                .collect()
        })
        .unwrap_or_default();
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "usb_write_count": plan.len(), "changes": plan,
        "predicted_after_hex": state::hex(&predicted.0), "unrestored_byte_offsets": ignored,
        "note": "Only mapped mixer fields are restorable. Opaque bytes, input trims, INST, 48 V and LED settings are not replayed." })).map_err(|e| e.to_string())?);
    Ok(())
}

fn run(cli: Cli) -> Result<(), String> {
    // Validate offline files before opening hardware. Show never opens a node.
    let saved = match &cli.command {
        Commands::Config {
            action:
                ConfigAction::Diff {
                    file,
                    against: Some(against),
                    mix,
                },
        } => {
            let current = Snapshot::load(against)?.state()?;
            let saved = Snapshot::load(file)?.state()?;
            return plan_json(
                &current,
                &mixer::restore(&current, &saved, *mix)?,
                Some(&saved),
            );
        }
        Commands::Config {
            action: ConfigAction::Show { file, json },
        } => {
            let snapshot = Snapshot::load(file)?;
            return show(&snapshot.device, &snapshot.state()?, *json);
        }
        Commands::Config {
            action: ConfigAction::Diff { file, .. } | ConfigAction::Restore { file, .. },
        } => Some(Snapshot::load(file)?.state()?),
        _ => None,
    };
    let (mutating, dry_run) = match &cli.command {
        Commands::Mixer { action, .. } => (
            true,
            match action {
                MixerAction::Channel { dry_run, .. }
                | MixerAction::Master { dry_run, .. }
                | MixerAction::Source { dry_run, .. } => *dry_run,
            },
        ),
        Commands::Route { dry_run, .. } => (true, *dry_run),
        Commands::Config {
            action: ConfigAction::Restore { dry_run, .. },
        } => (true, *dry_run),
        _ => (false, false),
    };
    let mut kernel = transport::Kernel::open(mutating && !dry_run)?;
    let (device, state) = kernel.read()?;
    let changes = match cli.command {
        Commands::Status { json } => return show(&device, &state, json),
        Commands::Config {
            action: ConfigAction::Save { file },
        } => {
            Snapshot::new(device, &state).save(&file)?;
            println!(
                "Saved {}. This is a host snapshot; no USB write or device-flash save command was sent.",
                file.display()
            );
            return Ok(());
        }
        Commands::Config {
            action: ConfigAction::Diff { mix, .. },
        } => {
            return plan_json(
                &state,
                &mixer::restore(&state, saved.as_ref().unwrap(), mix)?,
                saved.as_ref(),
            );
        }
        Commands::Config {
            action: ConfigAction::Restore { mix, .. },
        } => mixer::restore(&state, saved.as_ref().unwrap(), mix)?,
        Commands::Mixer {
            mix,
            action:
                MixerAction::Channel {
                    source,
                    level,
                    pan,
                    mute,
                    solo,
                    ..
                },
        } => mixer::channel(
            &state,
            mix,
            &source,
            level.as_deref(),
            pan,
            mute.map(Toggle::enabled),
            solo.map(Toggle::enabled),
        )?,
        Commands::Mixer {
            mix,
            action: MixerAction::Master { level, .. },
        } => mixer::master(mix, &level)?,
        Commands::Mixer {
            mix,
            action:
                MixerAction::Source {
                    source,
                    state: included,
                    ..
                },
        } => mixer::source(&state, mix, &source, included.enabled())?,
        Commands::Route { mix, source, .. } => mixer::route(mix, &source)?,
        Commands::Config {
            action: ConfigAction::Show { .. },
        } => unreachable!(),
    };
    let changes = state::changed(&state, changes)?;
    if dry_run {
        return plan_json(&state, &changes, saved.as_ref());
    }
    if let Some(saved) = &saved {
        let expected = state::predict(&state, &changes)?;
        let ignored = (0..state::STATE_SIZE)
            .filter(|&i| saved.0[i] != expected.0[i])
            .count();
        if ignored > 0 {
            eprintln!(
                "note: {ignored} snapshot bytes are outside the selected/restorable fields and will retain their live values; inspect config diff for offsets."
            );
        }
    }
    let dir = cli.backup_dir.unwrap_or_else(config::default_backup_dir);
    match config::apply(&mut kernel, &device, &state, changes, &dir)? {
        Some(path) => println!(
            "MiniFuse 4: change verified by complete B2 readback. Operation record: {}",
            path.display()
        ),
        None => println!("MiniFuse 4: already matches; no USB writes sent."),
    }
    Ok(())
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_default();
    if !arg.starts_with("--backup-dir=")
        && !matches!(
            arg.as_str(),
            "status"
                | "mixer"
                | "route"
                | "config"
                | "help"
                | "--help"
                | "-h"
                | "--version"
                | "-V"
                | "--backup-dir"
        )
    {
        legacy::run();
        return;
    }
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    #[test]
    fn parses_negative_levels_and_offline_commands() {
        for args in [
            vec![
                "mf-cli", "mixer", "out12", "channel", "analog1", "--level", "-6", "--mute", "on",
            ],
            vec!["mf-cli", "mixer", "loopback", "master", "-inf", "--dry-run"],
            vec!["mf-cli", "config", "show", "test.json", "--json"],
            vec![
                "mf-cli",
                "config",
                "restore",
                "test.json",
                "--mix",
                "out34",
                "--dry-run",
            ],
        ] {
            assert!(Cli::try_parse_from(args).is_ok());
        }
        assert!(Cli::try_parse_from(["mf-cli", "mixer", "out56", "master", "0"]).is_err());
    }
}
