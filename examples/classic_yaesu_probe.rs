//! Model-backed classic five-byte Yaesu CAT validation probe.
//!
//! Usage: `cargo run --example classic_yaesu_probe -- MODEL PORT BAUD [--exercise]`

use anyhow::{Context, Result};
use futures::executor::block_on;
use rigwright::{LegacyYaesuRadio, Radio, YaesuLegacyModel};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let model_name = args
        .next()
        .context("usage: classic_yaesu_probe MODEL PORT BAUD")?;
    let port = args
        .next()
        .context("usage: classic_yaesu_probe MODEL PORT BAUD")?;
    let baud: u32 = args.next().context("BAUD must be an integer")?.parse()?;
    let exercise = args.next().as_deref() == Some("--exercise");
    anyhow::ensure!(
        args.next().is_none(),
        "usage: classic_yaesu_probe MODEL PORT BAUD [--exercise]"
    );
    let model = YaesuLegacyModel::from_model_name(&model_name)
        .with_context(|| format!("unsupported classic Yaesu model: {model_name}"))?;
    let radio = LegacyYaesuRadio::new_for_model(model, port.clone(), baud)?;
    let frequency = block_on(radio.get_frequency_hz())?;
    let mode = block_on(radio.get_mode())?;
    let rx = radio.get_rx_status()?;
    let tx = radio.get_tx_status()?;
    let split = radio.get_split()?;
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
        block_on(radio.set_frequency_hz(frequency))?;
        block_on(radio.set_mode(mode))?;
        radio.set_split(split)?;
        radio.toggle_vfo()?;
        radio.toggle_vfo()?;
    }
    println!("PTT enable, CAT lock, and raw writes: skipped (operator-impacting)");
    println!("transport metrics: {:?}", radio.transport_metrics());
    Ok(())
}
