//! Interactive, read-only-first serial radio probe wizard.
//!
//! Start with:
//! `cargo run --example probe_wizard --`
//!
//! Use `--port`, `--model`, and `--baud` to make the flow non-interactive.
//! PTT is never tested unless `--ptt` is supplied and the operator confirms it.

use std::{
    fmt::Debug,
    fs,
    io::{self, Write},
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, ensure, Context, Result};
use futures::executor::block_on;
use rigwright::{
    drivers::{open_model_with_radio_address, ConfiguredRadio},
    enumerate_serial_port_descriptors,
    models::{find_model, Protocol, RadioModelProfile, POPULAR_RADIOS},
    ControlId, ControlValue, MeterId, Radio, RadioCapabilities,
};

#[derive(Debug, Default)]
struct Options {
    port: Option<String>,
    model: Option<String>,
    baud: Option<u32>,
    ptt: bool,
    log: Option<String>,
    exercise: Option<String>,
    exercise_writes: bool,
    list_only: bool,
}

#[derive(Debug)]
struct Candidate {
    model: Option<String>,
    protocol: Option<Protocol>,
    description: String,
    baud: Option<u32>,
    radio_address: Option<u8>,
}

#[derive(Debug)]
struct ModelSelection {
    model: String,
    baud: Option<u32>,
    radio_address: Option<u8>,
}

fn main() -> Result<()> {
    let options = parse_options()?;
    if options.list_only {
        print_devices()?;
        return Ok(());
    }

    let descriptors = enumerate_serial_port_descriptors()
        .context("failed to enumerate available USB/serial devices")?;
    ensure!(
        !descriptors.is_empty(),
        "no serial devices found; connect the radio interface and try again"
    );

    let port = match options.port.as_deref() {
        Some(port) => port.to_owned(),
        None => choose_port(&descriptors)?,
    };
    let descriptor = descriptors.iter().find(|item| item.port_name == port);
    println!("\nSelected device: {port}");
    if let Some(descriptor) = descriptor {
        println!("USB/serial details: {}", descriptor.display_name);
    }

    let selection = match options.model {
        Some(model) => ModelSelection {
            model: canonical_model(&model)?.model.to_owned(),
            baud: None,
            radio_address: None,
        },
        None => choose_model(
            &port,
            descriptor.and_then(|item| item.likely_radio.as_deref()),
        )?,
    };
    let profile = canonical_model(&selection.model)?;
    let baud = options
        .baud
        .or(selection.baud)
        .unwrap_or_else(|| profile.preferred_baud_rate());
    ensure!(
        profile.supported_baud_rates().contains(&baud),
        "baud {baud} is not documented for {}; choose one of {:?}",
        profile.model,
        profile.supported_baud_rates()
    );

    println!(
        "\nSelected radio: {} ({}) @ {baud} baud",
        profile.model,
        profile.protocol.label()
    );
    let radio = open_model_with_radio_address(
        profile.model,
        port.clone(),
        baud,
        0xE0,
        selection.radio_address,
    )?;
    run_read_only_probe(&radio, profile)?;

    if options.ptt {
        confirm_ptt()?;
        run_safe_ptt_test(&radio)?;
    } else {
        println!("\nPTT test: skipped (read-only mode; use --ptt to enable)");
    }

    if let Some(path) = options.exercise {
        run_exercise(&radio, profile, &path, options.exercise_writes)?;
        println!("diagnostic report: {path}");
    }

    if let Some(path) = options.log {
        let mut report =
            rigwright::probe::ProbeLog::new("probe_wizard", profile.model, &port, baud);
        report.pass("selected device", &port);
        report.pass("selected model", profile.model);
        report.skip(
            "PTT",
            if options.ptt {
                "operator-requested PTT test completed separately"
            } else {
                "read-only wizard run"
            },
        );
        report
            .write(&path)
            .with_context(|| format!("writing probe log {path}"))?;
        println!("probe log: {path}");
    }

    Ok(())
}

fn parse_options() -> Result<Options> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--list" => options.list_only = true,
            "--ptt" => options.ptt = true,
            "--exercise" => options.exercise = Some(required_value(&mut args, "--exercise")?),
            "--exercise-writes" => options.exercise_writes = true,
            "--port" => options.port = Some(required_value(&mut args, "--port")?),
            "--model" => options.model = Some(required_value(&mut args, "--model")?),
            "--baud" => {
                options.baud = Some(
                    required_value(&mut args, "--baud")?
                        .parse()
                        .context("--baud must be an integer")?,
                )
            }
            "--log" => options.log = Some(required_value(&mut args, "--log")?),
            other => bail!("unknown argument {other}; use --help for usage"),
        }
    }
    Ok(options)
}

fn required_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    args.next()
        .with_context(|| format!("{flag} requires a value"))
}

fn print_help() {
    println!(
        "Usage: cargo run --example probe_wizard -- [OPTIONS]\n\n\
         Options:\n\
           --list              List available serial devices and exit\n\
           --port PATH         Select a device without prompting\n\
           --model MODEL       Select a catalog model without prompting\n\
           --baud RATE         Override the model's preferred baud rate\n\
           --ptt               Enable the explicit minimum-power PTT test\n\
           --exercise PATH     Exercise the selected driver's safe HAL surface and write a report\n\
           --exercise-writes   Round-trip safe writable controls (never PTT or raw protocol)\n\
           --log PATH          Write a probe report\n\
           -h, --help          Show this help"
    );
}

fn print_devices() -> Result<()> {
    let descriptors = enumerate_serial_port_descriptors()?;
    if descriptors.is_empty() {
        println!("No serial devices found.");
    } else {
        for (index, descriptor) in descriptors.iter().enumerate() {
            println!(
                "{}: {}{}",
                index + 1,
                descriptor.display_name,
                descriptor
                    .likely_radio
                    .as_deref()
                    .map(|model| format!(" — hint: {model}"))
                    .unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn choose_port(descriptors: &[rigwright::SerialPortDescriptor]) -> Result<String> {
    println!("Available USB/serial devices:");
    for (index, descriptor) in descriptors.iter().enumerate() {
        println!(
            "  {}. {}{}",
            index + 1,
            descriptor.display_name,
            descriptor
                .likely_radio
                .as_deref()
                .map(|model| format!(" — hint: {model}"))
                .unwrap_or_default()
        );
    }
    let choice = prompt("Select a device number")?;
    let index: usize = choice
        .parse()
        .context("device selection must be a number")?;
    descriptors
        .get(
            index
                .checked_sub(1)
                .context("device selection starts at 1")?,
        )
        .map(|descriptor| descriptor.port_name.clone())
        .context("device selection is out of range")
}

fn choose_model(port: &str, usb_hint: Option<&str>) -> Result<ModelSelection> {
    let candidates = identify_candidates(port, usb_hint);
    if candidates.is_empty() {
        println!("\nNo protocol-level identification response was confirmed.");
    } else {
        println!("\nIdentification candidates:");
        for (index, candidate) in candidates.iter().enumerate() {
            println!(
                "  {}. {}{}",
                index + 1,
                candidate.description,
                candidate
                    .baud
                    .map(|baud| format!(" @ {baud} baud"))
                    .unwrap_or_default()
            );
        }
        let choice =
            prompt("Choose a candidate number, or press Enter for manual model selection")?;
        if !choice.is_empty() {
            let index: usize = choice
                .parse()
                .context("candidate selection must be a number")?;
            if let Some(candidate) =
                candidates.get(index.checked_sub(1).context("selection starts at 1")?)
            {
                if let Some(model) = &candidate.model {
                    return Ok(ModelSelection {
                        model: model.clone(),
                        baud: candidate.baud,
                        radio_address: candidate.radio_address,
                    });
                }
                return choose_catalog(candidate.protocol, candidate.baud, candidate.radio_address);
            }
        }
    }

    choose_catalog(None, None, None)
}

fn choose_catalog(
    protocol: Option<Protocol>,
    baud: Option<u32>,
    radio_address: Option<u8>,
) -> Result<ModelSelection> {
    println!("\nCatalog models:");
    for profile in POPULAR_RADIOS.iter().filter(|profile| {
        protocol.is_none_or(|expected| same_protocol_family(profile.protocol, expected))
            && !profile.model.to_ascii_lowercase().contains("generic")
    }) {
        println!("  {:<14} {}", profile.model, profile.protocol.label());
    }
    loop {
        let model = prompt("Enter the catalog model (for example FT-1000D)")?;
        if let Some(profile) = find_model(&model) {
            if protocol.is_none_or(|expected| same_protocol_family(profile.protocol, expected))
                && !profile.model.to_ascii_lowercase().contains("generic")
            {
                return Ok(ModelSelection {
                    model: profile.model.to_owned(),
                    baud,
                    radio_address,
                });
            }
        }
        println!("Unknown or incompatible catalog model: {model}");
    }
}

fn canonical_model(model: &str) -> Result<&'static RadioModelProfile> {
    find_model(model).with_context(|| format!("unknown catalog model: {model}"))
}

fn same_protocol_family(left: Protocol, right: Protocol) -> bool {
    matches!(
        (left, right),
        (Protocol::IcomCiV { .. }, Protocol::IcomCiV { .. })
            | (Protocol::YaesuCat, Protocol::YaesuCat)
            | (Protocol::YaesuLegacyCat, Protocol::YaesuLegacyCat)
            | (Protocol::KenwoodCat, Protocol::KenwoodCat)
            | (Protocol::ElecraftCat, Protocol::ElecraftCat)
    )
}

fn identify_candidates(port: &str, usb_hint: Option<&str>) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    if let Some(hint) = usb_hint {
        if let Some(profile) = POPULAR_RADIOS
            .iter()
            .find(|profile| hint.contains(profile.model))
        {
            candidates.push(Candidate {
                model: Some(profile.model.to_owned()),
                protocol: Some(profile.protocol),
                description: format!("USB identity suggests {}", profile.model),
                baud: Some(profile.preferred_baud_rate()),
                radio_address: None,
            });
        } else {
            candidates.push(Candidate {
                model: None,
                protocol: None,
                description: hint.to_owned(),
                baud: None,
                radio_address: None,
            });
        }
    }

    // These are bounded, read-only generic checks. Legacy Yaesu CAT is not
    // included because its protocol has no identification command.
    let kenwood = rigwright::KenwoodCatRadio::new_generic(port, 9_600);
    if kenwood.verify_model().is_ok() {
        candidates.push(Candidate {
            model: None,
            protocol: Some(Protocol::KenwoodCat),
            description: "Kenwood PC control responded to ID".to_owned(),
            baud: Some(9_600),
            radio_address: None,
        });
    }
    let elecraft = rigwright::ElecraftRadio::new_generic(port, 9_600);
    if elecraft.identify().is_ok() {
        candidates.push(Candidate {
            model: None,
            protocol: Some(Protocol::ElecraftCat),
            description: "Elecraft CAT responded to ID".to_owned(),
            baud: Some(9_600),
            radio_address: None,
        });
    }
    let yaesu = rigwright::YaesuCatRadio::new_generic(port, 38_400);
    if block_on(yaesu.get_frequency_hz()).is_ok() {
        candidates.push(Candidate {
            model: None,
            protocol: Some(Protocol::YaesuCat),
            description: "modern Yaesu CAT returned a frequency".to_owned(),
            baud: Some(38_400),
            radio_address: None,
        });
    }

    let icom = rigwright::IcomCiVRadio::new_generic(port, 9_600, 0xE0, 0x94);
    let addresses = POPULAR_RADIOS
        .iter()
        .filter_map(|profile| match profile.protocol {
            Protocol::IcomCiV { default_address } => Some(default_address),
            _ => None,
        })
        .collect::<Vec<_>>();
    println!("\nProbing Icom CI-V addresses and documented baud rates (read-only)...");
    if let Ok(result) = icom.probe_candidates(
        &[115_200, 19_200, 9_600, 4_800, 38_400, 57_600, 1_200, 300],
        &addresses,
    ) {
        let matching_models = POPULAR_RADIOS
            .iter()
            .filter(|profile| {
                profile.protocol
                    == Protocol::IcomCiV {
                        default_address: result.radio_address,
                    }
            })
            .filter(|profile| !profile.model.to_ascii_lowercase().contains("generic"))
            .collect::<Vec<_>>();
        let model = (matching_models.len() == 1).then(|| matching_models[0].model.to_owned());
        let description = model.as_deref().map_or_else(
            || {
                format!(
                    "Icom CI-V responded (exact model not reported; address {:#04x})",
                    result.radio_address
                )
            },
            |model| {
                format!("Icom CI-V matched documented default address for {model} (verify model)")
            },
        );
        candidates.push(Candidate {
            model,
            protocol: Some(Protocol::IcomCiV {
                default_address: result.radio_address,
            }),
            description,
            baud: Some(result.baud_rate),
            radio_address: Some(result.radio_address),
        });
    }
    candidates
}

fn run_read_only_probe(radio: &ConfiguredRadio, profile: &RadioModelProfile) -> Result<()> {
    println!("\nRead-only probe:");
    block_on(radio.probe()).context("connectivity/identity probe failed")?;
    let frequency = block_on(radio.get_frequency_hz()).context("frequency read failed")?;
    let mode = block_on(radio.get_mode()).context("mode read failed")?;
    println!("  PASS frequency: {frequency} Hz");
    println!("  PASS mode: {mode:?}");
    if radio.capabilities().can_get_ptt {
        println!("  PASS PTT: {}", block_on(radio.get_ptt())?);
    } else {
        println!("  SKIP PTT read: unsupported by {}", profile.model);
    }
    for meter in [rigwright::MeterId::Signal, rigwright::MeterId::Power] {
        if radio.supports_meter(meter) {
            println!("  PASS {meter:?}: {:?}", block_on(radio.get_meter(meter))?);
        }
    }
    Ok(())
}

fn run_exercise(
    radio: &ConfiguredRadio,
    profile: &RadioModelProfile,
    path: &str,
    exercise_writes: bool,
) -> Result<()> {
    let started_at = std::time::SystemTime::now();
    let mut report = String::new();
    report.push_str("# Rigwright driver diagnostic\n\n");
    report.push_str(&format!("- Model: `{}`\n", profile.model));
    report.push_str(&format!("- Protocol: `{}`\n", profile.protocol.label()));
    report.push_str(&format!("- Started: `{:?}`\n", started_at));
    report.push_str(&format!(
        "- Writable exercise enabled: `{exercise_writes}`\n\n"
    ));
    report.push_str("Each operation below is an actual HAL call. `SKIP` means the selected profile does not advertise the operation; `FAIL` includes the driver error returned by the command.\n\n");
    report.push_str("| Operation | Result | Duration | Value or reason |\n|---|---|---:|---|\n");

    record_call(&mut report, "probe", || block_on(radio.probe()));
    record_call(&mut report, "get_frequency_hz", || {
        block_on(radio.get_frequency_hz())
    });
    let mode_started = Instant::now();
    let current_mode = match block_on(radio.get_mode()) {
        Ok(mode) => {
            record_result(&mut report, "get_mode", mode_started.elapsed(), Ok(mode));
            Some(mode)
        }
        Err(error) => {
            record_result(
                &mut report,
                "get_mode",
                mode_started.elapsed(),
                Err::<rigwright::Mode, _>(error),
            );
            None
        }
    };
    record_call(&mut report, "read_core_state", || {
        block_on(radio.read_core_state())
    });
    if exercise_writes {
        match block_on(radio.get_frequency_hz()) {
            Ok(frequency) => record_call(&mut report, "set_frequency_hz (same value)", || {
                block_on(radio.set_frequency_hz(frequency))
            }),
            Err(error) => record_result(
                &mut report,
                "set_frequency_hz (same value)",
                Duration::ZERO,
                Err::<(), _>(error),
            ),
        }
        match block_on(radio.get_mode()) {
            Ok(mode) => record_call(&mut report, "set_mode (same value)", || {
                block_on(radio.set_mode(mode))
            }),
            Err(error) => record_result(
                &mut report,
                "set_mode (same value)",
                Duration::ZERO,
                Err::<(), _>(error),
            ),
        }
    } else {
        skipped(&mut report, "set_frequency_hz", "write exercise disabled");
        skipped(&mut report, "set_mode", "write exercise disabled");
    }

    let capabilities = radio.capabilities();
    report.push_str("\n## Capabilities\n\n");
    report.push_str(&format!("`{capabilities:?}`\n\n"));
    if capabilities.can_get_ptt {
        record_call(&mut report, "get_ptt", || block_on(radio.get_ptt()));
    } else {
        skipped(&mut report, "get_ptt", "profile capability is false");
    }
    if capabilities.can_get_power {
        record_call(&mut report, "get_power", || block_on(radio.get_power()));
    } else {
        skipped(&mut report, "get_power", "profile capability is false");
    }
    if capabilities.can_set_power {
        skipped(
            &mut report,
            "set_power",
            "power-state write requires an explicit operator policy",
        );
    } else {
        skipped(&mut report, "set_power", "profile capability is false");
    }

    report.push_str("\n## Meters\n\n");
    for meter in MeterId::ALL {
        if radio.supports_meter(*meter) {
            record_call(&mut report, &format!("get_meter::{meter:?}"), || {
                block_on(radio.get_meter(*meter))
            });
        } else {
            skipped(
                &mut report,
                &format!("get_meter::{meter:?}"),
                "not advertised by profile",
            );
        }
    }

    report.push_str("\n## Controls\n\n");
    for control in ControlId::ALL {
        let label = format!("control::{control:?}");
        if !radio.supports_control(*control) {
            skipped(&mut report, &label, "not advertised by profile");
            continue;
        }
        report.push_str(&format!("### `{control:?}`\n\n"));
        report.push_str(&format!(
            "- read: `{}`\n",
            radio.supports_control_read(*control)
        ));
        report.push_str(&format!(
            "- write: `{}`\n",
            radio.supports_control_write(*control)
        ));
        report.push_str(&format!("- max: `{:?}`\n", radio.control_max(*control)));
        report.push_str(&format!(
            "- values: `{:?}`\n\n",
            radio.supported_control_values(*control)
        ));
        if radio.supports_control_read(*control) {
            let control_started = Instant::now();
            let value = block_on(radio.get_control(*control));
            match value {
                Ok(value) => {
                    record_result(
                        &mut report,
                        &format!("get_control::{control:?}"),
                        control_started.elapsed(),
                        Ok(value.clone()),
                    );
                    if exercise_writes && radio.supports_control_write(*control) {
                        if is_safe_round_trip_control(*control) {
                            if let Some(value) = value {
                                record_call(
                                    &mut report,
                                    &format!("set_control::{control:?} (same value)"),
                                    || block_on(radio.set_control(*control, value.clone())),
                                );
                                record_call(
                                    &mut report,
                                    &format!("get_control::{control:?} (readback)"),
                                    || block_on(radio.get_control(*control)),
                                );
                            } else {
                                skipped(
                                    &mut report,
                                    &format!("set_control::{control:?}"),
                                    "no readable value to restore",
                                );
                            }
                        } else {
                            skipped(
                                &mut report,
                                &format!("set_control::{control:?}"),
                                "side effect requires explicit dedicated test",
                            );
                        }
                    }
                }
                Err(error) => {
                    record_result(
                        &mut report,
                        &format!("get_control::{control:?}"),
                        control_started.elapsed(),
                        Err::<Option<ControlValue>, _>(error),
                    );
                }
            }
        } else {
            skipped(
                &mut report,
                &format!("get_control::{control:?}"),
                "readback unsupported",
            );
        }
    }

    report.push_str("\n## Optional HAL surfaces\n\n");
    if current_mode == Some(rigwright::Mode::Data) {
        skipped(
            &mut report,
            "get_repeater_settings",
            "IC-7300 manual does not make RIT/repeater controls available in Data mode",
        );
    } else if radio.supports_repeater_settings() {
        let repeater_started = Instant::now();
        match block_on(radio.get_repeater_settings()) {
            Ok(settings) => {
                record_result(
                    &mut report,
                    "get_repeater_settings",
                    repeater_started.elapsed(),
                    Ok(settings),
                );
                if exercise_writes {
                    record_call(&mut report, "set_repeater_settings (same value)", || {
                        block_on(radio.set_repeater_settings(settings))
                    });
                } else {
                    skipped(
                        &mut report,
                        "set_repeater_settings",
                        "write exercise disabled",
                    );
                }
            }
            Err(error) => record_result(
                &mut report,
                "get_repeater_settings",
                repeater_started.elapsed(),
                Err::<rigwright::RepeaterSettings, _>(error),
            ),
        }
    } else {
        skipped(
            &mut report,
            "get_repeater_settings",
            "profile does not advertise repeater settings",
        );
    }
    for (control, name) in [(ControlId::Rit, "rit"), (ControlId::Xit, "xit")] {
        if control == ControlId::Rit && current_mode == Some(rigwright::Mode::Data) {
            skipped(
                &mut report,
                "get_rit_offset_hz",
                "IC-7300 manual does not make RIT available in Data mode",
            );
            continue;
        }
        if !radio.supports_control_read(control) {
            skipped(
                &mut report,
                &format!("get_{name}_offset_hz"),
                "offset readback not advertised",
            );
            continue;
        }
        let offset_started = Instant::now();
        let offset = if control == ControlId::Rit {
            block_on(radio.get_rit_offset_hz())
        } else {
            block_on(radio.get_xit_offset_hz())
        };
        match offset {
            Ok(offset) => {
                record_result(
                    &mut report,
                    &format!("get_{name}_offset_hz"),
                    offset_started.elapsed(),
                    Ok(offset),
                );
                if exercise_writes {
                    if control == ControlId::Rit {
                        record_call(&mut report, "set_rit_offset_hz (same value)", || {
                            block_on(radio.set_rit_offset_hz(offset))
                        });
                    } else {
                        record_call(&mut report, "set_xit_offset_hz (same value)", || {
                            block_on(radio.set_xit_offset_hz(offset))
                        });
                    }
                } else {
                    skipped(
                        &mut report,
                        &format!("set_{name}_offset_hz"),
                        "write exercise disabled",
                    );
                }
            }
            Err(error) => record_result(
                &mut report,
                &format!("get_{name}_offset_hz"),
                offset_started.elapsed(),
                Err::<i32, _>(error),
            ),
        }
    }
    if radio.supports_memory_channels() {
        skipped(
            &mut report,
            "select/read/write memory channel",
            "not exercised: selecting or writing an arbitrary channel changes radio state",
        );
    } else {
        skipped(
            &mut report,
            "select/read/write memory channel",
            "profile does not advertise memory channels",
        );
    }
    if radio.supports_scope() {
        record_call(&mut report, "get_scope_state", || {
            block_on(radio.get_scope_state())
        });
    } else {
        skipped(
            &mut report,
            "get_scope_state",
            "profile does not advertise native scope",
        );
    }
    if radio.supports_control_read(ControlId::Tuner) {
        record_call(&mut report, "get_tuner_status", || {
            block_on(radio.get_tuner_status())
        });
    } else {
        skipped(
            &mut report,
            "get_tuner_status",
            "tuner status is not advertised",
        );
    }
    skipped(
        &mut report,
        "start_tuner",
        "side effect can transmit or retune; no generic safe test",
    );
    skipped(
        &mut report,
        "set_scope_configuration",
        "configuration changes the radio display/stream; no generic same-value fixture",
    );
    skipped(
        &mut report,
        "set_ptt",
        "transmit-affecting; use --ptt for the dedicated minimum-power test",
    );
    skipped(
        &mut report,
        "protocol_write_read",
        "raw protocol operation requires a model-specific request",
    );
    skipped(
        &mut report,
        "send_dtmf",
        "side effect; no generic safe test sequence",
    );

    report.push_str("\n## Link health after exercise\n\n");
    report.push_str(&format!("`{:?}`\n", radio.link_health()));
    fs::write(path, report).with_context(|| format!("writing diagnostic report {path}"))?;
    Ok(())
}

fn record_call<T: Debug, F: FnOnce() -> Result<T>>(report: &mut String, operation: &str, call: F) {
    let started = Instant::now();
    let result = call();
    record_result(report, operation, started.elapsed(), result);
}

fn record_result<T: Debug>(
    report: &mut String,
    operation: &str,
    duration: Duration,
    result: Result<T>,
) {
    let (status, detail) = match result {
        Ok(value) => ("PASS", format!("`{value:?}`")),
        Err(error) if is_unsupported_error(&error) => (
            "SKIP",
            format!("unsupported: {}", error).replace('|', "\\|"),
        ),
        Err(error) => ("FAIL", format_error(&error)),
    };
    report.push_str(&format!(
        "| `{operation}` | {status} | {} ms | {} |\n",
        duration.as_millis(),
        detail.replace('|', "\\|")
    ));
}

fn is_unsupported_error(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("not supported")
        || message.contains("not available")
        || message.contains("unsupported")
}

fn skipped(report: &mut String, operation: &str, reason: &str) {
    report.push_str(&format!(
        "| `{operation}` | SKIP | — | {} |\n",
        reason.replace('|', "\\|")
    ));
}

fn format_error(error: &anyhow::Error) -> String {
    format!("FAIL: {}", error).replace('|', "\\|")
}

fn is_safe_round_trip_control(control: ControlId) -> bool {
    !matches!(
        control,
        ControlId::RfPower
            | ControlId::Tuner
            | ControlId::RawCiV
            | ControlId::Antenna
            | ControlId::Vfo
            | ControlId::MainSub
            | ControlId::Lock
    )
}

fn confirm_ptt() -> Result<()> {
    println!("\nWARNING: this will key the transmitter briefly at normalized minimum RF power.");
    let answer = prompt("Type PTT to continue")?;
    ensure!(answer == "PTT", "PTT test cancelled");
    Ok(())
}

fn run_safe_ptt_test(radio: &ConfiguredRadio) -> Result<()> {
    let capabilities: RadioCapabilities = radio.capabilities();
    ensure!(
        capabilities.can_set_ptt,
        "this radio does not support PTT writes"
    );
    ensure!(
        radio.supports_control_write(ControlId::RfPower),
        "this profile does not expose a writable RF-power minimum; PTT test refused"
    );
    let previous_power = block_on(radio.get_control(ControlId::RfPower))?
        .context("RF-power readback is required so the wizard can restore it")?;
    ensure!(
        matches!(previous_power, ControlValue::U8(_)),
        "RF-power readback was not a normalized numeric value"
    );
    if capabilities.can_get_ptt && block_on(radio.get_ptt())? {
        bail!("radio is already transmitting; refusing to start the PTT test");
    }

    println!("PTT test: setting normalized RF power to 0 (profile minimum)");
    block_on(radio.set_control(ControlId::RfPower, ControlValue::U8(0)))?;
    let test_result = (|| -> Result<()> {
        block_on(radio.set_ptt(true))?;
        thread::sleep(Duration::from_millis(300));
        ensure!(
            block_on(radio.get_ptt())?,
            "radio did not report transmitting"
        );
        println!("  PASS PTT asserted");
        Ok(())
    })();

    let dekey_result = block_on(radio.set_ptt(false));
    if dekey_result.is_ok() {
        println!("  PASS PTT de-keyed");
    }
    let restore_result = block_on(radio.set_control(ControlId::RfPower, previous_power));
    if restore_result.is_ok() {
        println!("  PASS RF power restored");
    }
    test_result.and(dekey_result).and(restore_result)
}

fn prompt(message: &str) -> Result<String> {
    print!("{message}: ");
    io::stdout().flush().context("flush prompt")?;
    let mut input = String::new();
    io::stdin().read_line(&mut input).context("read prompt")?;
    Ok(input.trim().to_owned())
}
