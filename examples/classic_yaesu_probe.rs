//! Model-backed classic five-byte Yaesu CAT validation probe.
//!
//! Usage: `cargo run --example classic_yaesu_probe -- MODEL PORT BAUD [--exercise]`

use anyhow::{Context, Result};
use futures::executor::block_on;
use rigwright::{probe::ProbeLog, LegacyYaesuRadio, Radio, YaesuLegacyModel};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let model_name = args
        .next()
        .context("usage: classic_yaesu_probe MODEL PORT BAUD")?;
    let port = args
        .next()
        .context("usage: classic_yaesu_probe MODEL PORT BAUD")?;
    let baud: u32 = args.next().context("BAUD must be an integer")?.parse()?;
    let flags: Vec<_> = args.collect();
    let exercise = flags.iter().any(|flag| flag == "--exercise");
    let log_path = flags
        .windows(2)
        .find(|pair| pair[0] == "--log")
        .map(|pair| pair[1].clone());
    anyhow::ensure!(
        flags.iter().enumerate().all(|(index, flag)| {
            flag == "--exercise" || flag == "--log" || (index > 0 && flags[index - 1] == "--log")
        }),
        "usage: classic_yaesu_probe MODEL PORT BAUD [--exercise] [--log PATH]"
    );
    anyhow::ensure!(
        !flags.iter().any(|flag| flag == "--log") || log_path.is_some(),
        "--log requires a PATH"
    );
    let model = YaesuLegacyModel::from_model_name(&model_name)
        .with_context(|| format!("unsupported classic Yaesu model: {model_name}"))?;
    let radio = LegacyYaesuRadio::new_for_model(model, port.clone(), baud)?;
    let mut report = ProbeLog::new("classic_yaesu_probe", model.model_name(), &port, baud);
    let frequency = block_on(radio.get_frequency_hz())?;
    let mode = block_on(radio.get_mode())?;
    let rx = radio.get_rx_status()?;
    let tx = radio.get_tx_status()?;
    let split = radio.get_split()?;
    report.pass("frequency", frequency);
    report.pass("mode", format!("{mode:?}"));
    report.pass("PTT", tx.transmitting);
    report.pass("split", split);
    println!(
        "{} {port} @ {baud}: {frequency} Hz, {mode:?}",
        model.model_name()
    );
    println!(
        "PTT={}, split={}, S={}/15, power={}/15, high SWR={}",
        tx.transmitting, split, rx.s_meter, tx.power_meter, tx.high_swr
    );
    println!("serial policy: {:?}", radio.serial_policy());
    if exercise {
        block_on(
            radio.set_frequency_hz(frequency.checked_add(1_000).context("frequency overflow")?),
        )?;
        block_on(radio.set_frequency_hz(frequency))?;
        let alternate_mode = if mode == rigwright::Mode::Usb {
            rigwright::Mode::Lsb
        } else {
            rigwright::Mode::Usb
        };
        block_on(radio.set_mode(alternate_mode))?;
        block_on(radio.set_mode(mode))?;
        radio.set_split(!split)?;
        radio.set_split(split)?;
        radio.toggle_vfo()?;
        radio.toggle_vfo()?;
    }
    println!("PTT enable, CAT lock, and raw writes: skipped (operator-impacting)");
    println!("transport metrics: {:?}", radio.transport_metrics());
    report.set_metrics(radio.transport_metrics());
    if let Some(path) = log_path {
        report
            .write(&path)
            .with_context(|| format!("writing probe log {path}"))?;
        println!("probe log: {path}");
    }
    Ok(())
}
