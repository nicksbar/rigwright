//! Interactive, read-only-first serial radio probe wizard.
//!
//! Start with:
//! `cargo run --example probe_wizard --`
//!
//! Use `--port`, `--model`, and `--baud` to make the flow non-interactive.
//! PTT is never tested unless `--ptt` is supplied and the operator confirms it.

use std::{
    io::{self, Write},
    thread,
    time::Duration,
};

use anyhow::{bail, ensure, Context, Result};
use futures::executor::block_on;
use rigwright::{
    drivers::{open_model, ConfiguredRadio},
    enumerate_serial_port_descriptors,
    models::{find_model, RadioModelProfile, POPULAR_RADIOS},
    ControlId, ControlValue, Radio, RadioCapabilities,
};

#[derive(Debug, Default)]
struct Options {
    port: Option<String>,
    model: Option<String>,
    baud: Option<u32>,
    ptt: bool,
    log: Option<String>,
    list_only: bool,
}

#[derive(Debug)]
struct Candidate {
    model: Option<String>,
    description: String,
    baud: Option<u32>,
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

    let selected_model = match options.model {
        Some(model) => canonical_model(&model)?.model.to_owned(),
        None => choose_model(
            &port,
            descriptor.and_then(|item| item.likely_radio.as_deref()),
        )?,
    };
    let profile = canonical_model(&selected_model)?;
    let baud = options
        .baud
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
    let radio = open_model(profile.model, port.clone(), baud, 0xE0)?;
    run_read_only_probe(&radio, profile)?;

    if options.ptt {
        confirm_ptt()?;
        run_safe_ptt_test(&radio)?;
    } else {
        println!("\nPTT test: skipped (read-only mode; use --ptt to enable)");
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

fn choose_model(port: &str, usb_hint: Option<&str>) -> Result<String> {
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
                    return Ok(model.clone());
                }
            }
        }
    }

    println!("\nCatalog models:");
    for profile in POPULAR_RADIOS.iter().filter(|profile| {
        profile.model != "CI-V (generic)"
            && profile.model != "CAT (generic)"
            && profile.model != "Classic CAT (generic)"
            && profile.model != "PC control (generic)"
    }) {
        println!("  {:<14} {}", profile.model, profile.protocol.label());
    }
    loop {
        let model = prompt("Enter the catalog model (for example FT-1000D)")?;
        if find_model(&model).is_some() {
            return Ok(model);
        }
        println!("Unknown catalog model: {model}");
    }
}

fn canonical_model(model: &str) -> Result<&'static RadioModelProfile> {
    find_model(model).with_context(|| format!("unknown catalog model: {model}"))
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
                description: format!("USB identity suggests {}", profile.model),
                baud: Some(profile.preferred_baud_rate()),
            });
        } else {
            candidates.push(Candidate {
                model: None,
                description: hint.to_owned(),
                baud: None,
            });
        }
    }

    // These are bounded, read-only generic checks. Legacy Yaesu CAT is not
    // included because its protocol has no identification command.
    let kenwood = rigwright::KenwoodCatRadio::new_generic(port, 9_600);
    if kenwood.verify_model().is_ok() {
        candidates.push(Candidate {
            model: None,
            description: "Kenwood PC control responded to ID".to_owned(),
            baud: Some(9_600),
        });
    }
    let elecraft = rigwright::ElecraftRadio::new_generic(port, 9_600);
    if elecraft.identify().is_ok() {
        candidates.push(Candidate {
            model: None,
            description: "Elecraft CAT responded to ID".to_owned(),
            baud: Some(9_600),
        });
    }
    let yaesu = rigwright::YaesuCatRadio::new_generic(port, 38_400);
    if block_on(yaesu.get_frequency_hz()).is_ok() {
        candidates.push(Candidate {
            model: None,
            description: "modern Yaesu CAT returned a frequency".to_owned(),
            baud: Some(38_400),
        });
    }
    let icom = rigwright::IcomCiVRadio::new_generic(port, 9_600, 0xE0, 0x94);
    if icom.probe().is_ok() {
        candidates.push(Candidate {
            model: None,
            description: "Icom CI-V returned frequency/mode at address 0x94".to_owned(),
            baud: Some(9_600),
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
