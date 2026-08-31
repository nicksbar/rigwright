//! Model-backed IC-7300 CI-V validation probe.
//!
//! Usage: `cargo run --example ci_v_probe -- [PORT] [BAUD] [--exercise]`

use anyhow::{Context, Result};
use futures::executor::block_on;
use rigwright::{probe::ProbeLog, ControlId, ControlValue, IcomCiVRadio, MeterId, Radio};

const CONTROLS: &[ControlId] = &[
    ControlId::Rit,
    ControlId::AfGain,
    ControlId::RfGain,
    ControlId::Squelch,
    ControlId::RfPower,
    ControlId::Preamp,
    ControlId::Attenuator,
    ControlId::NoiseBlanker,
    ControlId::NoiseReduction,
    ControlId::NoiseReductionLevel,
    ControlId::Notch,
    ControlId::ManualNotch,
    ControlId::ManualNotchPosition,
    ControlId::Tuner,
    ControlId::Split,
    ControlId::Xit,
    ControlId::Agc,
    ControlId::IpPlus,
    ControlId::DataMode,
    ControlId::Filter,
    ControlId::Vfo,
];
const METERS: &[MeterId] = &[
    MeterId::Signal,
    MeterId::Power,
    MeterId::Swr,
    MeterId::Alc,
    MeterId::Compression,
    MeterId::Voltage,
    MeterId::Current,
];

fn alternate_control(id: ControlId, value: &ControlValue) -> Option<ControlValue> {
    match value {
        ControlValue::Bool(value) => Some(ControlValue::Bool(!value)),
        ControlValue::U8(value) => {
            let alternate = match id {
                ControlId::Attenuator => {
                    if *value == 0 {
                        20
                    } else {
                        0
                    }
                }
                ControlId::Preamp => {
                    if *value == 0 {
                        1
                    } else {
                        0
                    }
                }
                ControlId::Agc => {
                    if *value == 2 {
                        1
                    } else {
                        2
                    }
                }
                ControlId::Filter => match *value {
                    1 => 2,
                    2 => 3,
                    _ => 1,
                },
                _ => {
                    if *value == 0 {
                        1
                    } else {
                        0
                    }
                }
            };
            Some(ControlValue::U8(alternate))
        }
        _ => None,
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let port = args.next().unwrap_or_else(|| "/dev/ttyUSB0".to_string());
    let baud: u32 = args
        .next()
        .unwrap_or_else(|| "115200".to_string())
        .parse()
        .context("BAUD must be an integer")?;
    let flags: Vec<_> = args.collect();
    let exercise = flags.iter().any(|flag| flag == "--exercise");
    let restore_rit_off = flags.iter().any(|flag| flag == "--restore-rit-off");
    let log_path = flags
        .windows(2)
        .find(|pair| pair[0] == "--log")
        .map(|pair| pair[1].clone());
    anyhow::ensure!(
        flags.iter().enumerate().all(|(index, flag)| {
            flag == "--exercise"
                || flag == "--restore-rit-off"
                || flag == "--log"
                || (index > 0 && flags[index - 1] == "--log")
        }),
        "usage: ci_v_probe [PORT] [BAUD] [--exercise] [--restore-rit-off] [--log PATH]"
    );
    anyhow::ensure!(
        !flags.iter().any(|flag| flag == "--log") || log_path.is_some(),
        "--log requires a PATH"
    );
    let radio = IcomCiVRadio::new_for_model_default_address(
        rigwright::models::IcomCivModel::Ic7300,
        port.clone(),
        baud,
        0xE0,
    );
    let mut report = ProbeLog::new("ci_v_probe", "IC-7300", &port, baud);
    let frequency = block_on(radio.get_frequency_hz())?;
    let mode = block_on(radio.get_mode())?;
    let ptt = block_on(radio.get_ptt())?;
    println!("IC-7300 {port} @ {baud}: {frequency} Hz, {mode:?}, PTT={ptt}");
    report.pass("frequency", frequency);
    report.pass("mode", format!("{mode:?}"));
    report.pass("PTT", ptt);
    if restore_rit_off {
        block_on(radio.set_control(ControlId::Rit, ControlValue::Bool(false)))?;
        println!("RIT restored OFF");
        report.pass("RIT restore", "OFF");
        report.set_metrics(radio.transport_metrics());
        if let Some(path) = log_path {
            report
                .write(&path)
                .with_context(|| format!("writing probe log {path}"))?;
            println!("probe log: {path}");
        }
        return Ok(());
    }
    let mut failures = 0;
    println!("tuner status: {:?}", block_on(radio.get_tuner_status())?);
    if exercise {
        block_on(radio.set_frequency_hz(frequency))?;
        // Keep the current operating mode unchanged while exercising controls.
        block_on(radio.set_mode(mode))?;
    }
    for &id in CONTROLS {
        if !radio.supports_control(id) {
            continue;
        }
        match block_on(radio.get_control(id)) {
            Ok(value) => {
                println!("control {id:?}: {value:?}");
                if exercise && radio.supports_control_write(id) {
                    if let Some(value) = value {
                        if let Some(alternate) = alternate_control(id, &value) {
                            if let Err(error) = block_on(radio.set_control(id, alternate)) {
                                eprintln!("control {id:?} alternate write failed: {error}");
                                failures += 1;
                            }
                        }
                        if let Err(error) = block_on(radio.set_control(id, value)) {
                            eprintln!("control {id:?} restore failed: {error}");
                            failures += 1;
                        }
                    }
                }
            }
            Err(error) if id == ControlId::Vfo => {
                println!("control {id:?}: write-only by IC-7300 profile ({error})");
            }
            Err(error) => {
                eprintln!("control {id:?} read failed: {error}");
                failures += 1;
            }
        }
    }
    for &id in METERS {
        if radio.supports_meter(id) {
            match block_on(radio.get_meter(id)) {
                Ok(value) => println!("meter {id:?}: {value:?}"),
                Err(error) => {
                    eprintln!("meter {id:?} read failed: {error}");
                    failures += 1;
                }
            }
        }
    }
    let repeater = match radio.get_repeater_settings() {
        Ok(value) => {
            println!("repeater: {value:?}");
            Some(value)
        }
        Err(error) => {
            println!("repeater: unavailable in current radio mode ({error})");
            None
        }
    };
    let mut rit_was_enabled = false;
    let mut rit = None;
    if mode == rigwright::Mode::Data {
        println!("RIT: unavailable while IC-7300 is in Data mode");
    } else {
        rit_was_enabled = match block_on(radio.get_control(ControlId::Rit))? {
            Some(ControlValue::Bool(value)) => value,
            other => anyhow::bail!("unexpected RIT enable value: {other:?}"),
        };
        if !rit_was_enabled {
            block_on(radio.set_control(ControlId::Rit, ControlValue::Bool(true)))?;
        }
        rit = match radio.get_rit_offset_hz() {
            Ok(value) => {
                println!("RIT offset: {value} Hz");
                Some(value)
            }
            Err(error) => {
                println!("RIT offset unavailable ({error})");
                None
            }
        };
    }
    if exercise {
        if let Some(value) = repeater {
            if let Err(error) = radio.set_repeater_settings(value) {
                eprintln!("repeater write-back failed: {error}");
                failures += 1;
            }
        }
        if let Some(value) = rit {
            if let Err(error) = radio.set_rit_offset_hz(value) {
                eprintln!("RIT write-back failed: {error}");
                failures += 1;
            }
        }
        if mode != rigwright::Mode::Data && !rit_was_enabled {
            block_on(radio.set_control(ControlId::Rit, ControlValue::Bool(false)))?;
        }
    } else if mode != rigwright::Mode::Data && !rit_was_enabled {
        block_on(radio.set_control(ControlId::Rit, ControlValue::Bool(false)))?;
    }
    println!("memory read/write: skipped (would alter operator memory)");
    println!("PTT/tuner start/scope stream: skipped (operator-impacting)");
    println!("transport metrics: {:?}", radio.transport_metrics());
    report.skip("memory read/write", "would alter operator memory");
    report.skip("PTT/tuner start/scope stream", "operator-impacting");
    report.set_metrics(radio.transport_metrics());
    if let Some(path) = log_path {
        report
            .write(&path)
            .with_context(|| format!("writing probe log {path}"))?;
        println!("probe log: {path}");
    }
    anyhow::ensure!(failures == 0, "{failures} IC-7300 checks failed");
    Ok(())
}
