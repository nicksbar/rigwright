//! Emit the generated Rigwright support/evidence matrix as JSON.
//!
//! Usage: `cargo run --example support_matrix -- [--pretty|--markdown]`

use rigwright::SupportMatrix;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args
        .iter()
        .any(|arg| !matches!(arg.as_str(), "--pretty" | "--markdown"))
        || args.contains(&"--pretty".to_owned()) && args.contains(&"--markdown".to_owned())
    {
        eprintln!("usage: support_matrix [--pretty|--markdown]");
        std::process::exit(2);
    }
    let matrix = SupportMatrix::from_catalog();
    if args.iter().any(|arg| arg == "--markdown") {
        print!("{}", matrix.to_markdown());
    } else {
        println!(
            "{}",
            matrix.to_json(args.iter().any(|arg| arg == "--pretty"))?
        );
    }
    Ok(())
}
