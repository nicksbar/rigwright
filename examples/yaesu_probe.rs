//! Read-only modern Yaesu CAT probe.
//!
//! Usage: `cargo run --example yaesu_probe -- FTDX10 /dev/ttyUSB0 38400 [--hardware-flow]`

use anyhow::{Context, Result};
use rigwright::{probe::ProbeLog, Radio, YaesuCatModel, YaesuCatRadio};

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
    let flags: Vec<_> = arguments.collect();
    let hardware_flow_control = flags.iter().any(|flag| flag == "--hardware-flow");
    let log_path = flags
        .windows(2)
        .find(|pair| pair[0] == "--log")
        .map(|pair| pair[1].clone());
    anyhow::ensure!(
        flags.iter().enumerate().all(|(index, flag)| {
            flag == "--hardware-flow"
                || flag == "--log"
                || (index > 0 && flags[index - 1] == "--log")
        }),
        "usage: yaesu_probe MODEL SERIAL_PORT BAUD [--hardware-flow] [--log PATH]"
    );

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
    let mut report = ProbeLog::new("yaesu_probe", model.model_name(), &port, baud_rate);

    let mut failures = Vec::new();
    run_check("identity", &mut failures, &mut report, || {
        radio.verify_model()
    });

    match futures::executor::block_on(radio.get_frequency_hz()) {
        Ok(frequency_hz) => {
            println!("PASS frequency: {frequency_hz} Hz");
            report.pass("frequency", frequency_hz);
        }
        Err(error) => {
            println!("FAIL frequency: {error:#}");
            failures.push(format!("frequency: {error:#}"));
            report.fail("frequency", format!("{error:#}"));
        }
    }

    match futures::executor::block_on(radio.get_mode()) {
        Ok(mode) => {
            println!("PASS mode: {mode:?}");
            report.pass("mode", format!("{mode:?}"));
        }
        Err(error) => {
            println!("FAIL mode: {error:#}");
            failures.push(format!("mode: {error:#}"));
            report.fail("mode", format!("{error:#}"));
        }
    }

    match futures::executor::block_on(radio.get_ptt()) {
        Ok(transmitting) => {
            println!("PASS PTT: {transmitting}");
            report.pass("PTT", transmitting);
        }
        Err(error) => {
            println!("FAIL PTT: {error:#}");
            failures.push(format!("PTT: {error:#}"));
            report.fail("PTT", format!("{error:#}"));
        }
    }

    report.set_metrics(radio.transport_metrics());
    if let Some(path) = log_path {
        report
            .write(&path)
            .with_context(|| format!("writing probe log {path}"))?;
        println!("probe log: {path}");
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

fn run_check<F>(name: &str, failures: &mut Vec<String>, report: &mut ProbeLog, check: F)
where
    F: FnOnce() -> anyhow::Result<()>,
{
    match check() {
        Ok(()) => {
            println!("PASS {name}");
            report.pass(name, "ok");
        }
        Err(error) => {
            println!("FAIL {name}: {error:#}");
            failures.push(format!("{name}: {error:#}"));
            report.fail(name, format!("{error:#}"));
        }
    }
}
