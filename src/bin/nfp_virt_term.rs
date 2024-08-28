use clap::Parser;
use clap_num::maybe_hex;

use rust_nfp_tools::libs::common::validate_nfp_bdf;
use rust_nfp_tools::libs::cpp_bus::CppIsland;
use rust_nfp_tools::libs::expansion_bar::{init_device_bars, ExpansionBar};
use rust_nfp_tools::libs::rfpc::Rfpc;
use rust_nfp_tools::libs::virtual_terminal::VirtualTerminal;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ctrlc;

/// Struct representing the CLI arguments
#[derive(Parser, Debug)]
#[command(
    about = "Read from RFPC virtual terminal interface.",
    after_help = "Example usage - monitor virtual terminal at the top of ARM CTM \
                  (default location):\n \
                  nfp-virt-term -Z 0000:65:00.0 --isl=arm --address=0x3fef4\n\n \
                  Note: Only one receiver per virtual terminal memory instance \
                  may be used. The receiver functionality on the ARM island \
                  must be disabled."
)]
struct Cli {
    #[arg(short = 'Z', long = "pci-bdf", required = true, value_parser = validate_nfp_bdf)]
    pci_bdf: String,

    #[arg(short = 'i', long = "island", required = true)]
    island: CppIsland,

    #[arg(short = 'a', long = "address", default_value = "0x3fef4", value_parser = maybe_hex::<u32>)]
    address: u32,
}

fn main() {
    let cli = Cli::parse();

    // Initialize the PCIe BARs in the PCIe config space.
    init_device_bars(&cli.pci_bdf);

    // Allocate a new expansion BAR for the PCIe device.
    let mut exp_bar = ExpansionBar::new(&cli.pci_bdf, None);
    let mut virt_term = VirtualTerminal::new(&mut exp_bar, cli.island, cli.address);
    let mut prev_holder: Option<Rfpc> = None;

    // Use an atomic flag to handle ctrl+c termination
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    println!(
        "Polling virtual terminal on device {} \
        in island {} memory \
        at address {}.\n \
        Press ctrl+C to escape.",
        &cli.pci_bdf, cli.island, cli.address
    );

    // Handle ctrl+c to gracefully exit
    ctrlc::set_handler(move || {
        println!("\n\nKeyboard interrupt received (ctrl+C). Exiting.");
        r.store(false, Ordering::SeqCst); // Set running to false
    })
    .expect("Error setting Ctrl-C handler");

    // Main loop
    while running.load(Ordering::SeqCst) {
        let cur_holder: Option<Rfpc> = virt_term.holder();

        if prev_holder != cur_holder {
            if let Some(prev) = &prev_holder {
                // Print closing footer for previous holder
                println!("</{}>", prev);
            }

            if let Some(current) = &cur_holder {
                // Print opening header for current holder
                println!("<{}>", current);
            }

            prev_holder = cur_holder.clone();
        }

        if cur_holder.is_none() {
            // Virtual terminal not held, nothing to do.
            thread::sleep(Duration::from_millis(100)); // Polling interval
            continue;
        }

        if virt_term.data_available() != 0 {
            print!("{}", virt_term.read_string());
        }

        thread::sleep(Duration::from_millis(100)); // Polling interval
    }
}
