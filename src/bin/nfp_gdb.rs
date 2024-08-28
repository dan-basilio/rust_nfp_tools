use clap::Parser;

use rust_nfp_tools::libs::common::validate_nfp_bdf;
use rust_nfp_tools::libs::expansion_bar::{init_device_bars, ExpansionBar};
use rust_nfp_tools::libs::explicit_bar::ExplicitBar;
use rust_nfp_tools::libs::gdb_server_stub::RspServer;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ctrlc;

/// Struct representing the CLI arguments
#[derive(Parser, Debug)]
#[command(
    about = "Start an RSP debug server to connect to an NFP RISC-V debugger.",
    long_about = None,
)]
struct Cli {
    #[arg(short = 'Z', long = "pci-bdf", required = true, value_parser = validate_nfp_bdf)]
    pci_bdf: String,
}

fn main() {
    let cli = Cli::parse();

    // Initialize the PCIe BARs in the PCIe config space.
    init_device_bars(&cli.pci_bdf);

    // Allocate a new expansion BAR for the PCIe device.
    let mut exp_bar = ExpansionBar::new(&cli.pci_bdf, None);
    // Allocate a new explicit BAR for the PCIe device.
    let mut expl_bar = ExplicitBar::new(&cli.pci_bdf, 0);

    // Use an atomic flag to handle ctrl+c termination.
    let running = Arc::new(AtomicBool::new(true));

    // Handle ctrl+c to gracefully exit.
    ctrlc::set_handler({
        let running = running.clone();
        move || {
            println!("\n\nKeyboard interrupt received (ctrl+C). Exiting.");
            running.store(false, Ordering::SeqCst);
        }
    })
    .expect("Error setting Ctrl-C handler");

    // Create an instance of RspServer.
    let mut rsp_server = RspServer::new(&mut exp_bar, &mut expl_bar);

    // Run the server in the main thread.
    rsp_server.run(running);

    println!("Server shutting down.");
}
