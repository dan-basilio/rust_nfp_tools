use clap::{ArgAction, Parser};
use clap_num::maybe_hex;

use rust_nfp_tools::libs::common::{hex_parser, validate_nfp_bdf};
use rust_nfp_tools::libs::cpp_bus::CppIsland;
use rust_nfp_tools::libs::expansion_bar::init_device_bars;
use rust_nfp_tools::libs::explicit_bar::ExplicitBar;
use rust_nfp_tools::libs::xpb_bus::{xpb_explicit_read32, xpb_explicit_write32};

/// Struct representing the CLI arguments
#[derive(Parser, Debug)]
#[command(
    about = "Read and write data on the CPP bus.",
    long_about = None,
    after_help = "Example usage - Read PCIE0 `pcie_pf0_k_pciconf0` register \
                 (address: `0x00B00040`): \n
                 nfp-xpb -Z 0000:65:00.0 --i=4 -a 0x00B00040 -l 1"
)]
struct Cli {
    #[arg(short = 'Z', long = "pci-bdf", required = true, value_parser = validate_nfp_bdf)]
    pci_bdf: String,

    #[arg(short = 'i', long = "island", required = true)]
    island: CppIsland,

    #[arg(short = 'a', long = "address", required = true, value_parser = maybe_hex::<u32>)]
    address: u32,

    #[arg(short = 'l', long = "length", default_value_t = 1, value_parser = maybe_hex::<u64>)]
    length: u64,

    #[arg(short = 'v', long = "value", action = ArgAction::Append, num_args = 1.., value_parser = hex_parser)]
    values: Vec<u32>,

    #[arg(short = 'x', long = "xpbm", action = ArgAction::SetTrue)]
    xpbm: bool,
}

fn main() {
    let cli = Cli::parse();

    // Initialize the PCIe BARs in the PCIe config. space.
    init_device_bars(&cli.pci_bdf);

    // Allocate a new explicit BAR for the PCIe device.
    let mut expl_bar = ExplicitBar::new(&cli.pci_bdf, 0);

    if cli.values.is_empty() {
        // Read over Xpb bus.
        let read_word = xpb_explicit_read32(&mut expl_bar, &cli.island, cli.address, cli.xpbm);
        println!("0x{:08x}", read_word);
    } else {
        // Write over Xpb bus.
        xpb_explicit_write32(&mut expl_bar, &cli.island, cli.address, cli.values, cli.xpbm);
    }
}
