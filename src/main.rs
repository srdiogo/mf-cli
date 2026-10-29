// SPDX-License-Identifier: MIT
mod legacy;

use clap::{Parser, Subcommand, ValueEnum};
use mf_core::{self as core, Device, Error, Kernel, Mix, Operation, Plan, Snapshot, State};
use std::path::PathBuf;

fn mix_parser() -> impl clap::builder::TypedValueParser<Value = Mix> {
    use clap::builder::TypedValueParser;
    clap::builder::PossibleValuesParser::new(Mix::ALL.map(Mix::name))
        .map(|name| name.parse().expect("Known mix name"))
}

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
        #[arg(value_parser = mix_parser())]
        mix: Mix,
        #[command(subcommand)]
        action: MixerAction,
    },
    /// Route out12 (usb12/mix), out34 (usb34/mix), or loopback (usb56/mix/out12/out34)
    Route {
        #[arg(value_parser = mix_parser())]
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
        #[arg(long, value_parser = mix_parser())]
        mix: Option<Mix>,
    },
    /// Restore captured writable mixer fields; opaque bytes and trims stay unchanged
    Restore {
        file: PathBuf,
        #[arg(long, value_parser = mix_parser())]
        mix: Option<Mix>,
        #[arg(long)]
        dry_run: bool,
    },
}

fn show(device: &Device, state: &State, json: bool) -> Result<(), Error> {
    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "device": device, "state": state.decoded(), "payload_hex": core::hex(&state.0) })).map_err(|e| e.to_string())?);
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
            for (ch, name) in core::CHANNEL_NAMES.iter().enumerate().take(mix.channels()) {
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

fn plan_json(plan: &Plan) -> Result<(), Error> {
    println!(
        "{}",
        serde_json::to_string_pretty(&plan.summary()).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn run(cli: Cli) -> Result<(), Error> {
    let saved = match &cli.command {
        Commands::Config {
            action:
                ConfigAction::Diff {
                    file,
                    against: Some(against),
                    mix,
                },
        } => {
            let current = Snapshot::load(against)?;
            let saved = Snapshot::load(file)?;
            return plan_json(&core::plan_restore(
                &current.device,
                &current.state()?,
                &saved,
                *mix,
            )?);
        }
        Commands::Config {
            action: ConfigAction::Show { file, json },
        } => {
            let snapshot = Snapshot::load(file)?;
            return show(&snapshot.device, &snapshot.state()?, *json);
        }
        Commands::Config {
            action: ConfigAction::Diff { file, .. } | ConfigAction::Restore { file, .. },
        } => Some(Snapshot::load(file)?),
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
        Commands::Route { dry_run, .. }
        | Commands::Config {
            action: ConfigAction::Restore { dry_run, .. },
        } => (true, *dry_run),
        _ => (false, false),
    };
    let mut kernel = Kernel::open(mutating && !dry_run)?;
    let (device, state) = kernel.read()?;
    let operation = match cli.command {
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
            return plan_json(&core::plan_restore(
                &device,
                &state,
                saved.as_ref().unwrap(),
                mix,
            )?);
        }
        Commands::Config {
            action: ConfigAction::Restore { mix, .. },
        } => {
            let plan = core::plan_restore(&device, &state, saved.as_ref().unwrap(), mix)?;
            return apply(&mut kernel, &plan, dry_run, cli.backup_dir);
        }
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
        } => Operation::Channel {
            mix,
            source,
            level: level.map(|v| v.parse()).transpose()?,
            pan,
            mute: mute.map(Toggle::enabled),
            solo: solo.map(Toggle::enabled),
        },
        Commands::Mixer {
            mix,
            action: MixerAction::Master { level, .. },
        } => Operation::Master {
            mix,
            level: level.parse()?,
        },
        Commands::Mixer {
            mix,
            action:
                MixerAction::Source {
                    source,
                    state: included,
                    ..
                },
        } => Operation::Membership {
            mix,
            source,
            included: included.enabled(),
        },
        Commands::Route { mix, source, .. } => Operation::Route { mix, source },
        Commands::Config {
            action: ConfigAction::Show { .. },
        } => unreachable!(),
    };
    let plan = core::plan(&device, &state, &[operation])?;
    apply(&mut kernel, &plan, dry_run, cli.backup_dir)
}

fn apply(
    kernel: &mut Kernel,
    plan: &Plan,
    dry_run: bool,
    backup_dir: Option<PathBuf>,
) -> Result<(), Error> {
    if dry_run {
        return plan_json(plan);
    }
    if !plan.omitted_offsets().is_empty() {
        eprintln!(
            "note: {} snapshot bytes are outside the selected/restorable fields and will retain their live values; inspect config diff for offsets.",
            plan.omitted_offsets().len()
        );
    }
    let dir = backup_dir.unwrap_or_else(core::default_backup_dir);
    let outcome = core::execute(kernel, plan, &dir)?.require_success()?;
    match outcome.journal {
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
