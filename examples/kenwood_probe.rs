//! Read-only Kenwood PC-control probe.
//!
//! Usage: `cargo run --example kenwood_probe -- TS-590SG /dev/ttyUSB0 115200`

use anyhow::{Context, Result};
use rigwright::{KenwoodCatModel, KenwoodCatRadio, Radio};

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
    anyhow::ensure!(arguments.next().is_none(), "too many arguments");

    let model = KenwoodCatModel::from_model_name(&model_name)
        .with_context(|| format!("unsupported Kenwood model: {model_name}"))?;
    let radio = KenwoodCatRadio::new_for_model(model, port, baud_rate)?;

    radio.verify_model()?;
    let frequency_hz = futures::executor::block_on(radio.get_frequency_hz())?;
    let mode = futures::executor::block_on(radio.get_mode())?;
    let transmitting = if radio.capabilities().can_get_ptt {
        Some(futures::executor::block_on(radio.get_ptt())?)
    } else {
        None
    };

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
    Ok(())
}
