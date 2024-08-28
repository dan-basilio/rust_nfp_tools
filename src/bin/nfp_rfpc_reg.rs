use clap::{ArgGroup, Parser};
use clap_num::maybe_hex;

use rust_nfp_tools::libs::common::validate_nfp_bdf;
use rust_nfp_tools::libs::cpp_bus::CppIsland;
use rust_nfp_tools::libs::expansion_bar::init_device_bars;
use rust_nfp_tools::libs::explicit_bar::ExplicitBar;
use rust_nfp_tools::libs::rfpc::{Rfpc, RfpcCsr, RfpcGpr, RfpcReg};
use rust_nfp_tools::libs::rfpc_debugger::{read_rfpc_reg, write_rfpc_reg};

/// Struct representing the CLI arguments
#[derive(Parser, Debug)]
#[command(
    about = "Read and write RFPC registers (GPRs and CSRs).",
    long_about = None,
    after_help = "Example usage - read first RFPC's `mhartid` CSR:\n
                  nfp-rfpc-reg -Z 0000:65:00.0 --isl=rfpc0 --cluster=0 \
                  --group=0 --core=0 --csr=mhartid"
)]
#[command(group(ArgGroup::new("register")
    .required(true)
    .args(&["gpr", "csr"])))]
struct Cli {
    #[arg(short = 'Z', long = "pci-bdf", required = true, value_parser = validate_nfp_bdf)]
    pci_bdf: String,

    #[arg(short = 'i', long = "island", required = true)]
    island: CppIsland,

    #[arg(short = 'u', long = "cluster", required = true)]
    cluster: u8,

    #[arg(short = 'r', long = "group", required = true)]
    group: u8,

    #[arg(short = 'c', long = "core", required = true)]
    core: u8,

    #[arg(short = 's', long = "csr")]
    csr: Option<RfpcCsr>,

    #[arg(short = 'p', long = "gpr")]
    gpr: Option<RfpcGpr>,

    #[arg(short = 'v', long = "value", value_parser = maybe_hex::<u64>)]
    value: Option<u64>,
}

fn main() {
    let cli = Cli::parse();

    // Initialize the PCIe BARs in the PCIe config. space.
    init_device_bars(&cli.pci_bdf);

    // Allocate a new explicit BAR for the PCIe device.
    let mut expl_bar = ExplicitBar::new(&cli.pci_bdf, 0);

    let rfpc = Rfpc {
        island: cli.island,
        cluster: cli.cluster,
        group: cli.group,
        core: cli.core,
    };

    // Check whether we're dealing with a GPR or CSR register.
    let reg_addr: Box<dyn RfpcReg> = if let Some(csr_reg) = cli.csr {
        Box::new(csr_reg)
    } else if let Some(gpr_reg) = cli.gpr {
        Box::new(gpr_reg)
    } else {
        panic!("Error: Either CSR or GPR must be provided.");
    };

    if let Some(value) = cli.value {
        // Value provided - write to the register
        write_rfpc_reg(&mut expl_bar, &rfpc, &reg_addr, value);
    } else {
        // Read from the register
        let val = read_rfpc_reg(&mut expl_bar, &rfpc, &reg_addr);
        println!("{}:{} = 0x{:016x}", rfpc, reg_addr, val);
    }
}
