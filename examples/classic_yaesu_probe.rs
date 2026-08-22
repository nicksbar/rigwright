//! Read-only probe for classic five-byte Yaesu CAT radios.
//!
//! Usage: `cargo run --example classic_yaesu_probe -- FT-857D /dev/ttyUSB0 4800`

use anyhow::{Context, Result};
use rigwright::{LegacyYaesuRadio, Radio, YaesuLegacyModel};

fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let model_name = arguments
        .next()
        .context("usage: classic_yaesu_probe MODEL SERIAL_PORT BAUD")?;
    let port = arguments
        .next()
        .context("usage: classic_yaesu_probe MODEL SERIAL_PORT BAUD")?;
    let baud_rate: u32 = arguments
        .next()
        .context("usage: classic_yaesu_probe MODEL SERIAL_PORT BAUD")?
        .parse()
        .context("BAUD must be an integer")?;
    anyhow::ensure!(arguments.next().is_none(), "too many arguments");

    let model = YaesuLegacyModel::from_model_name(&model_name)
        .with_context(|| format!("unsupported classic Yaesu model: {model_name}"))?;
    let radio = LegacyYaesuRadio::new_for_model(model, port, baud_rate)?;

    let frequency_hz = futures::executor::block_on(radio.get_frequency_hz())?;
    let mode = futures::executor::block_on(radio.get_mode())?;
    let rx = radio.get_rx_status()?;
    let tx = radio.get_tx_status()?;

    println!("configured model: {}", model.model_name());
    println!("frequency: {frequency_hz} Hz");
    println!("mode: {mode:?}");
    println!("PTT: {}", if tx.transmitting { "TX" } else { "RX" });
    println!("split: {}", tx.split_enabled);
    println!("S meter: {}/15", rx.s_meter);
    println!("power meter: {}/15", tx.power_meter);
    println!("high SWR: {}", tx.high_swr);
    Ok(())
}
