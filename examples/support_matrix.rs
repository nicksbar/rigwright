//! Emit the generated Rigwright support/evidence matrix as JSON.
//!
//! Usage: `cargo run --example support_matrix -- [--pretty]`

use rigwright::SupportMatrix;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pretty = std::env::args().skip(1).any(|arg| arg == "--pretty");
    if std::env::args().skip(1).any(|arg| arg != "--pretty") {
        eprintln!("usage: support_matrix [--pretty]");
        std::process::exit(2);
    }
    println!("{}", SupportMatrix::from_catalog().to_json(pretty)?);
    Ok(())
}
