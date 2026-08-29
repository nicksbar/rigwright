//! Read-only modern Yaesu CAT probe.
//!
//! Usage: `cargo run --example yaesu_probe -- FTDX10 /dev/ttyUSB0 38400 [--hardware-flow]`

use anyhow::{Context, Result};
use rigwright::{Radio, YaesuCatModel, YaesuCatRadio};

fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let model_name = arguments
        .next()
        .context("usage: yaesu_probe MODEL SERIAL_PORT BAUD")?;
    let port = arguments
        .next()
        .context("usage: yaesu_probe MODEL SERIAL_PORT BAUD")?;
    let baud_rate: u32 = arguments
        .next()
        .context("usage: yaesu_probe MODEL SERIAL_PORT BAUD")?
        .parse()
        .context("BAUD must be an integer")?;
    let hardware_flow_control = match arguments.next() {
        None => false,
        Some(value) => {
            anyhow::ensure!(
                value == "--hardware-flow",
                "optional flag must be --hardware-flow"
            );
            true
        }
    };
    anyhow::ensure!(arguments.next().is_none(), "too many arguments");

    let model = YaesuCatModel::from_model_name(&model_name)
        .with_context(|| format!("unsupported modern Yaesu model: {model_name}"))?;
    let radio = if hardware_flow_control {
        YaesuCatRadio::new_for_model_with_hardware_flow_control(model, port, baud_rate)?
    } else {
        YaesuCatRadio::new_for_model(model, port, baud_rate)?
    };

    radio.verify_model()?;
    let frequency_hz = futures::executor::block_on(radio.get_frequency_hz())?;
    let mode = futures::executor::block_on(radio.get_mode())?;
    let transmitting = futures::executor::block_on(radio.get_ptt())?;

    println!("model: {}", model.model_name());
    println!("frequency: {frequency_hz} Hz");
    println!("mode: {mode:?}");
    println!("transmitting: {transmitting}");
    Ok(())
}
