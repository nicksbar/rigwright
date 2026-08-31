//! Read-only Kenwood PC-control probe.
//!
//! Usage: `cargo run --example kenwood_probe -- TS-590SG /dev/ttyUSB0 115200`

use anyhow::{Context, Result};
use rigwright::{probe::ProbeLog, KenwoodCatModel, KenwoodCatRadio, Radio};

fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let model_name = arguments
        .next()
        .context("usage: kenwood_probe MODEL SERIAL_PORT BAUD")?;
    let port = arguments
        .next()
        .context("usage: kenwood_probe MODEL SERIAL_PORT BAUD")?;
    let baud_rate: u32 = arguments
        .next()
        .context("usage: kenwood_probe MODEL SERIAL_PORT BAUD")?
        .parse()
        .context("BAUD must be an integer")?;
    let flags: Vec<_> = arguments.collect();
    let log_path = flags
        .windows(2)
        .find(|pair| pair[0] == "--log")
        .map(|pair| pair[1].clone());
    anyhow::ensure!(
        flags
            .iter()
            .enumerate()
            .all(|(index, flag)| { flag == "--log" || (index > 0 && flags[index - 1] == "--log") }),
        "usage: kenwood_probe MODEL SERIAL_PORT BAUD [--log PATH]"
    );
    anyhow::ensure!(
        !flags.iter().any(|flag| flag == "--log") || log_path.is_some(),
        "--log requires a PATH"
    );

    let model = KenwoodCatModel::from_model_name(&model_name)
        .with_context(|| format!("unsupported Kenwood model: {model_name}"))?;
    let radio = KenwoodCatRadio::new_for_model(model, port.clone(), baud_rate)?;
    let mut report = ProbeLog::new("kenwood_probe", model.model_name(), &port, baud_rate);

    radio.verify_model()?;
    report.pass("identity", "ok");
    let frequency_hz = futures::executor::block_on(radio.get_frequency_hz())?;
    report.pass("frequency", frequency_hz);
    let mode = futures::executor::block_on(radio.get_mode())?;
    report.pass("mode", format!("{mode:?}"));
    let transmitting = if radio.capabilities().can_get_ptt {
        Some(futures::executor::block_on(radio.get_ptt())?)
    } else {
        None
    };
    report.pass(
        "PTT",
        transmitting.map_or_else(|| "unavailable".to_owned(), |value| value.to_string()),
    );

    println!("model: {}", model.model_name());
    println!("frequency: {frequency_hz} Hz");
    println!("mode: {mode:?}");
    match transmitting {
        Some(value) => println!("PTT: {}", if value { "TX" } else { "RX" }),
        None => println!("PTT: not directly readable on this model"),
    }
    println!("split: {}", radio.get_split()?);
    println!("RF power setting: {} W", radio.get_power_watts()?);
    println!(
        "meter: {}/{}",
        radio.get_meter()?,
        radio.profile().expect("selected profile").meter_max
    );
    report.set_metrics(radio.transport_metrics());
    if let Some(path) = log_path {
        report
            .write(&path)
            .with_context(|| format!("writing probe log {path}"))?;
        println!("probe log: {path}");
    }
    Ok(())
}
