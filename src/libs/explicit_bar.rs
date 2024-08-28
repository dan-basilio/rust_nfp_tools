#![allow(dead_code)]

use crate::libs::cpp_bus::{CppIsland, CppLength};
use crate::libs::expansion_bar::{ExpansionBar, MapType, BAR_CONFIG_BASE_CONFIG_SNOOP};
use bytemuck::cast_slice;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};

// Number of explicit command BARs per PF.
const NUM_EXPL_BARS: u32 = 4;

// Offset of explicit command BAR CSRs from PCIe BAR config base address.
const CSR_EXPL_BASE_OFFSET: u32 = 0x180;
// Offset of explicit command BAR CSRs per explicit BAR.
const CSR_EXPL_BAR_OFFSET: u32 = 0x10;

// Base address of PCIe SRAM in PCIe internal target.
const PCIE_INT_SRAM_BASE: u32 = 0x40000;
// Offset of explicit command data transfer memory in PCIe SRAM.
const SRAM_DATA_BASE_OFFSET: u32 = 0xE000;
// Offset of explicit command data per explicit command BAR.
const SRAM_DATA_EXPL_BAR_OFFSET: u32 = 128;

pub struct ExplicitBar {
    pci_bdf: String,
    expl_bar_index: u32,
    trigger_exp_bar: ExpansionBar,
    data_exp_bar: ExpansionBar,
    expl_bar_cached_cfg: [u32; 4],
}

impl ExplicitBar {
    pub fn new(pci_bdf_str: &str, expl_bar_index: u32) -> Self {
        let mut trigger_exp_bar = ExpansionBar::new(pci_bdf_str, None);
        trigger_exp_bar.exp_bar_map = MapType::Explicit;
        // All fields are ignored when configuring the Explicit Bar.
        // The only relevant field is the MapType.
        trigger_exp_bar.expansion_bar_cfg(0, 0, 0, 0, 0, 0);
        let mut data_exp_bar = ExpansionBar::new(pci_bdf_str, None);
        data_exp_bar.exp_bar_map = MapType::General;
        data_exp_bar.expansion_bar_cfg(
            CppIsland::Local.id(),
            0, // Unused for General mapping
            0, // Unused for General mapping
            0, // Unused for General mapping
            (PCIE_INT_SRAM_BASE + SRAM_DATA_BASE_OFFSET) as u64,
            CppLength::Len32.id(),
        );

        ExplicitBar {
            pci_bdf: pci_bdf_str.to_string(),
            expl_bar_index,
            trigger_exp_bar,
            data_exp_bar,
            expl_bar_cached_cfg: [0; 4],
        }
    }

    pub fn expa_bar_offset(&self) -> u64 {
        (((self.trigger_exp_bar.exp_bar_size as u32) / NUM_EXPL_BARS) * self.expl_bar_index) as u64
    }

    pub fn size(&self) -> u64 {
        ((self.trigger_exp_bar.exp_bar_size as u32) / NUM_EXPL_BARS) as u64
    }

    pub fn csr_offset(&self) -> u64 {
        (CSR_EXPL_BASE_OFFSET + (self.expl_bar_index * CSR_EXPL_BAR_OFFSET)) as u64
    }

    pub fn sram_data_offset(&self) -> u64 {
        (PCIE_INT_SRAM_BASE
            + SRAM_DATA_BASE_OFFSET
            + (self.expl_bar_index * SRAM_DATA_EXPL_BAR_OFFSET)) as u64
    }

    fn expl_bar_config_write(&self, cfg_reg0: u32, cfg_reg1: u32, cfg_reg2: u32, cfg_reg3: u32) {
        let pcie_cfg_path = format!("/sys/bus/pci/devices/{}/config", &self.pci_bdf);

        let expl_bar_csr_addr = BAR_CONFIG_BASE_CONFIG_SNOOP as u64 + self.csr_offset();

        let mut pcie_cfg_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pcie_cfg_path)
            .expect(&format!("Failed to open file {}", &pcie_cfg_path));

        pcie_cfg_file
            .seek(SeekFrom::Start(expl_bar_csr_addr as u64))
            .expect(&format!("File {} seek failed", pcie_cfg_path));

        // Write using little-endian format
        pcie_cfg_file
            .write_all(&cfg_reg0.to_le_bytes())
            .expect(&format!("File {} write failed", &pcie_cfg_path));
        pcie_cfg_file
            .write_all(&cfg_reg1.to_le_bytes())
            .expect(&format!("File {} write failed", &pcie_cfg_path));
        pcie_cfg_file
            .write_all(&cfg_reg2.to_le_bytes())
            .expect(&format!("File {} write failed", &pcie_cfg_path));
        pcie_cfg_file
            .write_all(&cfg_reg3.to_le_bytes())
            .expect(&format!("File {} write failed", &pcie_cfg_path));
    }

    pub fn explicit_bar_cfg(
        &self,
        tgt_island_id: u8,
        target: u8,
        action: u8,
        token: u8,
        base_addr: u64,
        sig_type: Option<u8>,
        length: u8,
        byte_mask: u8,
        master_island: Option<u8>,
        data_master: Option<u8>,
        data_ref: Option<u8>,
        signal_master: Option<u8>,
        signal_ref: Option<u8>,
    ) {
        // Check if the optional input parameters are valid.
        if sig_type.is_some()
            && (master_island.is_some()
                || data_master.is_some()
                || data_ref.is_some()
                || signal_master.is_some()
                || signal_ref.is_some())
        {
            panic!(
                "sig_type must not be Some() if any of the master or \
                     reference parameters are Some()"
            );
        }

        if (0..16).contains(
            &((base_addr & base_addr.wrapping_neg()).leading_zeros() as u64).saturating_sub(1),
        ) {
            panic!(
                "Explicit command BARs use a 32-bit base address. \
                 The lower 16 bits of address {:#010x} would be truncated.",
                base_addr
            );
        }

        let (mut cfg0, mut cfg1, mut cfg2, mut cfg3): (u32, u32, u32, u32) = (0, 0, 0, 0);

        cfg0 |= (sig_type.unwrap_or(0) as u32 & 0x3) << 28; // Signal type field.
        cfg0 |= (action as u32 & 0x3F) << 20; // CPP action field.
        cfg0 |= (token as u32 & 0x3) << 16; // CPP token field.
        cfg0 |= (length as u32 & 0x1F) << 8; // CPP length field.
        cfg0 |= (byte_mask as u32 & 0xFF) << 0; // Byte mask field.

        cfg1 |= (target as u32 & 0xF) << 28; // CPP target field.
        cfg1 |= (master_island.unwrap_or(0) as u32 & 0x7F) << 21; // Master island field.
        cfg1 |= (data_master.unwrap_or(0) as u32 & 0x1F) << 16; // Data master field.
        cfg1 |= (data_ref.unwrap_or(0) as u32 & 0xFFFF) << 0; // Data reference field.

        cfg2 |= 1 << 31; // Enable bit.
        cfg2 |= (tgt_island_id as u32 & 0x7F) << 16; // Island/mode address field.
        cfg2 |= (signal_ref.unwrap_or(0) as u32 & 0x7F) << 8; // Signal reference field.
        cfg2 |= (signal_master.unwrap_or(0) as u32 & 0x1F) << 0; // Signal master field.

        cfg3 |= (base_addr >> 16) as u32 & 0xFFFFFFFF; // Base address field.

        self.expl_bar_config_write(cfg0, cfg1, cfg2, cfg3);
    }

    fn trigger(&self, offset: u64, length_words: u64) -> Vec<u32> {
        let length_bytes = length_words * 4;
        let read_bytes: Vec<u8> = self
            .trigger_exp_bar
            .read(self.expa_bar_offset() as u64 + offset, length_bytes);
        let read_words_slice: &[u32] = cast_slice(&read_bytes);
        read_words_slice.to_vec()
    }

    fn write_data(&mut self, data: Vec<u32>) {
        if data.len() > ((SRAM_DATA_EXPL_BAR_OFFSET / 4) as usize) {
            panic!("Length of data exceeds the SRAM size!");
        }

        let sram_addr = self.sram_data_offset();
        let write_bytes: Vec<u8> = cast_slice(&data).to_vec();
        self.data_exp_bar.write(&write_bytes, sram_addr);
    }

    fn read_data(&self, length_words: u64) -> Vec<u32> {
        if length_words > (SRAM_DATA_EXPL_BAR_OFFSET / 4).into() {
            panic!("Length of data exceeds the SRAM size!");
        }

        let sram_addr = self.sram_data_offset();
        let length_bytes: u64 = length_words * 4;
        let read_bytes = self.data_exp_bar.read(sram_addr, length_bytes);
        let read_words_slice: &[u32] = cast_slice(&read_bytes);
        read_words_slice.to_vec()
    }

    pub fn run_explicit_cmd(
        &mut self,
        offset: u64,
        pull_data: Option<Vec<u32>>,
        push_data_len: Option<u64>,
        require_push_data_from_sram: bool,
    ) -> Option<Vec<u32>> {
        // Write pull data if provided.
        if let Some(data) = pull_data {
            self.write_data(data);
        }

        // Constants for acceptable direct push data lengths.
        const VALID_DIRECT_SIZES: [u64; 3] = [1, 4, 8];

        // Determine if SRAM is required for push data.
        let use_sram = require_push_data_from_sram
            || push_data_len.map_or(true, |len| !VALID_DIRECT_SIZES.contains(&len));

        if use_sram {
            // Trigger explicit command by reading from expansion BAR.
            self.trigger(offset, 1);

            // If push_data_len is provided, read from SRAM.
            if let Some(len) = push_data_len {
                return Some(self.read_data(len));
            }
        } else {
            // Read directly from trigger expansion BAR.
            if let Some(len) = push_data_len {
                return Some(self.trigger(offset, len));
            }
        }

        None
    }
}
