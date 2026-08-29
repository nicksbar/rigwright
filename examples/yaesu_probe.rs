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
    println!("model: {}", model.model_name());
    println!("port: {port}");
    println!("baud: {baud_rate}");
    let radio = if hardware_flow_control {
        YaesuCatRadio::new_for_model_with_hardware_flow_control(model, port.clone(), baud_rate)?
    } else {
        YaesuCatRadio::new_for_model(model, port.clone(), baud_rate)?
    };

    let mut failures = Vec::new();
    run_check("identity", &mut failures, || radio.verify_model());

    match futures::executor::block_on(radio.get_frequency_hz()) {
        Ok(frequency_hz) => println!("PASS frequency: {frequency_hz} Hz"),
        Err(error) => {
            println!("FAIL frequency: {error:#}");
            failures.push(format!("frequency: {error:#}"));
        }
    }

    match futures::executor::block_on(radio.get_mode()) {
        Ok(mode) => println!("PASS mode: {mode:?}"),
        Err(error) => {
            println!("FAIL mode: {error:#}");
            failures.push(format!("mode: {error:#}"));
        }
    }

    match futures::executor::block_on(radio.get_ptt()) {
        Ok(transmitting) => println!("PASS PTT: {transmitting}"),
        Err(error) => {
            println!("FAIL PTT: {error:#}");
            failures.push(format!("PTT: {error:#}"));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!(
            "{} CAT check(s) failed after completing the probe: {}",
            failures.len(),
            failures.join("; ")
        )
    }
}

fn run_check<F>(name: &str, failures: &mut Vec<String>, check: F)
where
    F: FnOnce() -> anyhow::Result<()>,
{
    match check() {
        Ok(()) => println!("PASS {name}"),
        Err(error) => {
            println!("FAIL {name}: {error:#}");
            failures.push(format!("{name}: {error:#}"));
        }
    }
}
