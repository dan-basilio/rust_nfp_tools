#![allow(dead_code)]
use bytemuck::cast_slice;

use crate::libs::common::align_transaction64;
use crate::libs::explicit_bar::ExplicitBar;
use crate::libs::rfpc::{Rfpc, RfpcReg};
use crate::libs::xpb_bus::{xpb_explicit_read32, xpb_explicit_write32};

use std::thread;
use std::time::{Duration, Instant};

/// RISC-V DEBUG MODULE REGISTERS.
/// These are defined by the RISC-V debug standard, specified in section 3.12
/// of the document "RISC-V External Debug Support" version 0.13.2
/// (available from the RISC-V foundation website: https://riscv.org/).
/// Note: The provided address map for the DMI uses 32-bit word addresses,
/// whereas the NFP's XPB uses byte addresses. The DMI addresses are
/// therefore multiplied by 4 here to obtain the XPB addresses.
const RISCV_DBG_DATA0: u32 = 0x10;
const RISCV_DBG_DATA1: u32 = 0x14;
const RISCV_DBG_DATA2: u32 = 0x18;
const RISCV_DBG_DATA3: u32 = 0x1c;
const RISCV_DBG_DATA4: u32 = 0x20;
const RISCV_DBG_DATA5: u32 = 0x24;
const RISCV_DBG_DATA6: u32 = 0x28;
const RISCV_DBG_DATA7: u32 = 0x2c;
const RISCV_DBG_DATA8: u32 = 0x30;
const RISCV_DBG_DATA9: u32 = 0x34;
const RISCV_DBG_DATA10: u32 = 0x38;
const RISCV_DBG_DATA11: u32 = 0x3c;
const RISCV_DBG_DMCONTROL: u32 = 0x40;
const RISCV_DBG_DMSTATUS: u32 = 0x44;
const RISCV_DBG_HARTINFO: u32 = 0x48;
const RISCV_DBG_HALTSUM1: u32 = 0x4c;
const RISCV_DBG_HAWINDOWSEL: u32 = 0x50;
const RISCV_DBG_HAWINDOW: u32 = 0x54;
const RISCV_DBG_ABSTRACTCS: u32 = 0x58;
const RISCV_DBG_COMMAND: u32 = 0x5c;
const RISCV_DBG_ABSTRACTAUTO: u32 = 0x60;
const RISCV_DBG_CONFSTRPTR0: u32 = 0x64;
const RISCV_DBG_CONFSTRPTR1: u32 = 0x68;
const RISCV_DBG_CONFSTRPTR2: u32 = 0x6c;
const RISCV_DBG_CONFSTRPTR3: u32 = 0x70;
const RISCV_DBG_NEXTDM: u32 = 0x74;
const RISCV_DBG_PROGBUF0: u32 = 0x80;
const RISCV_DBG_PROGBUF1: u32 = 0x84;
const RISCV_DBG_PROGBUF2: u32 = 0x88;
const RISCV_DBG_PROGBUF3: u32 = 0x8c;
const RISCV_DBG_PROGBUF4: u32 = 0x90;
const RISCV_DBG_PROGBUF5: u32 = 0x94;
const RISCV_DBG_PROGBUF6: u32 = 0x98;
const RISCV_DBG_PROGBUF7: u32 = 0x9c;
const RISCV_DBG_PROGBUF8: u32 = 0xa0;
const RISCV_DBG_PROGBUF9: u32 = 0xa4;
const RISCV_DBG_PROGBUF10: u32 = 0xa8;
const RISCV_DBG_PROGBUF11: u32 = 0xac;
const RISCV_DBG_PROGBUF12: u32 = 0xb0;
const RISCV_DBG_PROGBUF13: u32 = 0xb4;
const RISCV_DBG_PROGBUF14: u32 = 0xb8;
const RISCV_DBG_PROGBUF15: u32 = 0xbc;
const RISCV_DBG_AUTHDATA: u32 = 0xc0;
const RISCV_DBG_HALTSUM2: u32 = 0xd0;
const RISCV_DBG_HALTSUM3: u32 = 0xd4;
const RISCV_DBG_SBADDRESS3: u32 = 0xdc;
const RISCV_DBG_SBCS: u32 = 0xe0;
const RISCV_DBG_SBADDRESS0: u32 = 0xe4;
const RISCV_DBG_SBADDRESS1: u32 = 0xe8;
const RISCV_DBG_SBADDRESS2: u32 = 0xec;
const RISCV_DBG_SBDATA0: u32 = 0xf0;
const RISCV_DBG_SBDATA1: u32 = 0xf4;
const RISCV_DBG_SBDATA2: u32 = 0xf8;
const RISCV_DBG_SBDATA3: u32 = 0xfc;
const RISCV_DBG_HALTSUM0: u32 = 0x100;

/// RISC-V DEBUG MODULE REGISTER FIELD MASKS.
const RISCV_DBG_DMCONTROL_HALTREQ: u32 = 1 << 31;
const RISCV_DBG_DMCONTROL_RESUMEREQ: u32 = 1 << 30;
const RISCV_DBG_DMCONTROL_HARTRESET: u32 = 1 << 29;
const RISCV_DBG_DMCONTROL_ACKHAVERESET: u32 = 1 << 28;
const RISCV_DBG_DMCONTROL_HASEL: u32 = 1 << 26;
const RISCV_DBG_DMCONTROL_HARTSELLO: u32 = 0x3FF << 16;
const RISCV_DBG_DMCONTROL_HARTSELHI: u32 = 0x3FF << 6;
const RISCV_DBG_DMCONTROL_SETRESETHALTREQ: u32 = 1 << 3;
const RISCV_DBG_DMCONTROL_CLRRESETHALTREQ: u32 = 1 << 2;
const RISCV_DBG_DMCONTROL_NDMRESET: u32 = 1 << 1;
const RISCV_DBG_DMCONTROL_DMACTIVE: u32 = 1 << 0;

const RISCV_DBG_DMSTATUS_IMPEBREAK: u32 = 1 << 22;
const RISCV_DBG_DMSTATUS_ALLHAVERESET: u32 = 1 << 19;
const RISCV_DBG_DMSTATUS_ANYHAVERESET: u32 = 1 << 18;
const RISCV_DBG_DMSTATUS_ALLRESUMEACK: u32 = 1 << 17;
const RISCV_DBG_DMSTATUS_ANYRESUMEACK: u32 = 1 << 16;
const RISCV_DBG_DMSTATUS_ALLNONEXISTENT: u32 = 1 << 15;
const RISCV_DBG_DMSTATUS_ANYNONEXISTENT: u32 = 1 << 14;
const RISCV_DBG_DMSTATUS_ALLUNAVAIL: u32 = 1 << 13;
const RISCV_DBG_DMSTATUS_ANYUNAVAIL: u32 = 1 << 12;
const RISCV_DBG_DMSTATUS_ALLRUNNING: u32 = 1 << 11;
const RISCV_DBG_DMSTATUS_ANYRUNNING: u32 = 1 << 10;
const RISCV_DBG_DMSTATUS_ALLHALTED: u32 = 1 << 9;
const RISCV_DBG_DMSTATUS_ANYHALTED: u32 = 1 << 8;
const RISCV_DBG_DMSTATUS_AUTHENTICATED: u32 = 1 << 7;
const RISCV_DBG_DMSTATUS_AUTHBUSY: u32 = 1 << 6;
const RISCV_DBG_DMSTATUS_HASRESETHALTREQ: u32 = 1 << 5;
const RISCV_DBG_DMSTATUS_CONFSTRPTRVALID: u32 = 1 << 4;
const RISCV_DBG_DMSTATUS_VERSION: u32 = 0xF;

const RISCV_DBG_ABSTRACTCS_PROGBUFSIZE: u32 = 0x1F << 24;
const RISCV_DBG_ABSTRACTCS_BUSY: u32 = 1 << 12;
const RISCV_DBG_ABSTRACTCS_CMDERR: u32 = 0x7 << 8;
const RISCV_DBG_ABSTRACTCS_DATACOUNT: u32 = 0xF;

pub fn read_rfpc_reg(expl_bar: &mut ExplicitBar, rfpc: &Rfpc, reg: &Box<dyn RfpcReg>) -> u64 {
    let reg_addr = reg.reg_addr();

    rfpc_dbg_halt(expl_bar, rfpc);
    let val = rfpc_dbg_read_reg(expl_bar, rfpc, reg_addr);
    rfpc_dbg_resume(expl_bar, rfpc);

    val
}

pub fn write_rfpc_reg(expl_bar: &mut ExplicitBar, rfpc: &Rfpc, reg: &Box<dyn RfpcReg>, value: u64) {
    let reg_addr = reg.reg_addr();

    rfpc_dbg_halt(expl_bar, rfpc);
    rfpc_dbg_write_reg(expl_bar, rfpc, reg_addr, value);
    rfpc_dbg_resume(expl_bar, rfpc);
}

pub fn rfpc_dbg_halt(expl_bar: &mut ExplicitBar, rfpc: &Rfpc) {
    let (hartsello, _) = rfpc.dm_hartsel();
    let mut dmcontrol = hartsello << 16;

    dmcontrol |= RISCV_DBG_DMCONTROL_DMACTIVE;
    dmcontrol |= RISCV_DBG_DMCONTROL_HALTREQ;

    // Write halt request to dmcontrol to initiate halt.
    xpb_explicit_write32(
        expl_bar,
        &rfpc.island,
        rfpc.dm_xpb_base() + RISCV_DBG_DMCONTROL,
        vec![dmcontrol],
        true,
    );

    // Poll dmstatus until RFPC is halted.
    let start_time = Instant::now();
    let timeout_duration = Duration::new(10, 0);
    loop {
        if start_time.elapsed() > timeout_duration {
            println!("Timeout reached in rfpc_dbg_halt()!");
            break;
        }

        let dmstatus = xpb_explicit_read32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_DMSTATUS,
            true,
        );
        if dmstatus & RISCV_DBG_DMSTATUS_ALLHALTED != 0 {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn rfpc_dbg_resume(expl_bar: &mut ExplicitBar, rfpc: &Rfpc) {
    let (hartsello, _) = rfpc.dm_hartsel();
    let mut dmcontrol = hartsello << 16;

    dmcontrol |= RISCV_DBG_DMCONTROL_DMACTIVE;
    dmcontrol |= RISCV_DBG_DMCONTROL_RESUMEREQ;

    // Write halt request to dmcontrol to initiate halt.
    xpb_explicit_write32(
        expl_bar,
        &rfpc.island,
        rfpc.dm_xpb_base() + RISCV_DBG_DMCONTROL,
        vec![dmcontrol],
        true,
    );

    // Poll dmstatus until RFPC is halted.
    let start_time = Instant::now();
    let timeout_duration = Duration::new(10, 0);
    loop {
        if start_time.elapsed() > timeout_duration {
            println!("Timeout reached in rfpc_dbg_resume()!");
            break;
        }
        let dmstatus = xpb_explicit_read32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_DMSTATUS,
            true,
        );
        if dmstatus & RISCV_DBG_DMSTATUS_ALLRUNNING != 0 {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn rfpc_dbg_abstractcmd(
    expl_bar: &mut ExplicitBar,
    rfpc: &Rfpc,
    cmdtype: u64,
    control: u64,
) -> u64 {
    let (hartsello, _) = rfpc.dm_hartsel();
    let mut dmcontrol = hartsello << 16;

    dmcontrol |= RISCV_DBG_DMCONTROL_DMACTIVE;

    xpb_explicit_write32(
        expl_bar,
        &rfpc.island,
        rfpc.dm_xpb_base() + RISCV_DBG_DMCONTROL,
        vec![dmcontrol],
        true,
    );

    // Do abstract command.
    let command = ((cmdtype & 0xFF) << 24) | (control & 0xFFFFFF);
    xpb_explicit_write32(
        expl_bar,
        &rfpc.island,
        rfpc.dm_xpb_base() + RISCV_DBG_COMMAND,
        vec![command as u32],
        true,
    );

    let mut abstractcs: u32 = 0;
    // Wait for command completion.
    let start_time = Instant::now();
    let timeout_duration = Duration::new(10, 0);
    loop {
        if start_time.elapsed() > timeout_duration {
            println!("Timeout reached in rfpc_dbg_abstractcmd()!");
            break;
        }
        abstractcs = xpb_explicit_read32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_ABSTRACTCS,
            true,
        );
        if (abstractcs & RISCV_DBG_ABSTRACTCS_BUSY) == 0 {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    if (abstractcs & RISCV_DBG_ABSTRACTCS_CMDERR) != 0 {
        // Clear error code if applicable.
        xpb_explicit_write32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_ABSTRACTCS,
            vec![RISCV_DBG_ABSTRACTCS_CMDERR],
            true,
        );
    }

    ((abstractcs & RISCV_DBG_ABSTRACTCS_CMDERR) >> 8).into()
}

pub fn rfpc_dbg_read_reg(expl_bar: &mut ExplicitBar, rfpc: &Rfpc, reg_addr: u64) -> u64 {
    let command = 0x320000 | (reg_addr & 0xFFFF);

    let err_code = rfpc_dbg_abstractcmd(expl_bar, rfpc, 0, command);
    if err_code != 0 {
        panic!("RFPC abstract command returned error {}.", err_code);
    }

    // Read the lower 32 bits of the register value.
    let mut reg_val: u64 = xpb_explicit_read32(
        expl_bar,
        &rfpc.island,
        rfpc.dm_xpb_base() + RISCV_DBG_DATA0,
        true,
    ) as u64;

    // Read the upper 32 bits of the register value.
    reg_val |= (xpb_explicit_read32(
        expl_bar,
        &rfpc.island,
        rfpc.dm_xpb_base() + RISCV_DBG_DATA1,
        true,
    ) as u64)
        << 32;

    reg_val
}

pub fn rfpc_dbg_write_reg(expl_bar: &mut ExplicitBar, rfpc: &Rfpc, reg_addr: u64, value: u64) {
    // Write lower 32 bits of register value to debug module data0.
    xpb_explicit_write32(
        expl_bar,
        &rfpc.island,
        rfpc.dm_xpb_base() + RISCV_DBG_DATA0,
        vec![(value & 0xFFFFFFFF) as u32],
        true,
    );

    // Write upper 32 bits of register value to debug module data1.
    xpb_explicit_write32(
        expl_bar,
        &rfpc.island,
        rfpc.dm_xpb_base() + RISCV_DBG_DATA1,
        vec![(value >> 32) as u32],
        true,
    );

    // Write the value in the debug module's data registers to the specified
    // RISC-V core register.
    let command = 0x330000 | (reg_addr & 0xFFFF);
    let err_code = rfpc_dbg_abstractcmd(expl_bar, rfpc, 0, command);
    if err_code != 0 {
        panic!("RFPC abstract command returned error {}.", err_code);
    }
}

pub fn rfpc_dbg_read_memory(
    expl_bar: &mut ExplicitBar,
    rfpc: &Rfpc,
    address: u64,
    length: u64,
) -> Vec<u32> {
    // Align address and length for 64-bit word access.
    let (align_addr, align_len) = align_transaction64(address, length);
    let word_len = align_len / 2; // Number of 64-bit words to read.

    // Save RFPC GPR a0 (x10) temporarily, as it will be overwritten for
    // the memory read process.
    let temp_a0 = rfpc_dbg_read_reg(expl_bar, rfpc, 0x100a);

    // Read from memory one 64-bit word at a time.
    let mut mem_words: Vec<u64> = Vec::new();
    for word_idx in 0..word_len {
        let word_addr = align_addr + 8 * word_idx; // Memory address of word.
                                                   // Write word memory address to debug module data0/1 registers.
        xpb_explicit_write32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_DATA0,
            vec![(word_addr & 0xFFFFFFFF) as u32],
            true,
        );
        xpb_explicit_write32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_DATA1,
            vec![(word_addr >> 32) as u32],
            true,
        );
        // Write load memory instruction to debug module progbuf0 register.
        // 0x53503 => `ld a0, (0)a0`  (load double word from mem[a0]).
        xpb_explicit_write32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_PROGBUF0,
            vec![0x53503],
            true,
        );
        // Execute abstract command: load ((data1 << 32) | data0) into RFPC
        // GPR a0 before executing the instruction in the program buffer.
        // This reads the 64-bit word in memory at word_addr into GPR a0.
        let err_code = rfpc_dbg_abstractcmd(expl_bar, rfpc, 0, 0x37100a);
        if err_code != 0 {
            panic!("RFPC abstract command returned error {}.", err_code);
        }
        // Read memory word from RFPC GPR a0.
        mem_words.push(rfpc_dbg_read_reg(expl_bar, rfpc, 0x100a));
    }

    // Restore RFPC GPR a0.
    rfpc_dbg_write_reg(expl_bar, rfpc, 0x100a, temp_a0);
    let mem_words_slice: &[u32] = cast_slice(&mem_words);

    mem_words_slice.to_vec()
}

pub fn rfpc_dbg_write_memory(
    expl_bar: &mut ExplicitBar,
    rfpc: &Rfpc,
    address: u64,
    data: Vec<u32>,
) {
    // Align address and length for 64-bit word access.
    let (align_addr, align_len) = align_transaction64(address, data.len() as u64);

    let mut new_data = Vec::new();

    if address != align_addr {
        // Read the initial padding bytes from memory.
        let prepend_len = if address > align_addr {
            address - align_addr
        } else {
            0
        };
        let prepend_data =
            rfpc_dbg_read_memory(expl_bar, rfpc, align_addr - prepend_len, prepend_len);
        new_data.extend(prepend_data);
    }

    // Add the original data after the prepended data
    new_data.extend(data);

    // Align the provided data to the required length by appending the final padding bytes.
    if (new_data.len() as u64) < align_len {
        let append_len = align_len - (new_data.len() as u64);
        let append_data = rfpc_dbg_read_memory(
            expl_bar,
            rfpc,
            align_addr + (new_data.len() as u64) * 4,
            append_len,
        );
        new_data.extend(append_data);
    }

    // Convert data from 32-bit words to 64-bit words.
    let mem_words: Vec<u64> = new_data
        .chunks(2)
        .map(|chunk| {
            let low = chunk.get(0).copied().unwrap_or(0) as u64;
            let high = chunk.get(1).copied().unwrap_or(0) as u64;
            (high << 32) | low
        })
        .collect();

    // Save RFPC GPRs a0 and a1 temporarily.
    let temp_a0 = rfpc_dbg_read_reg(expl_bar, rfpc, 0x100a);
    let temp_a1 = rfpc_dbg_read_reg(expl_bar, rfpc, 0x100b);

    for (word_idx, data_word) in mem_words.iter().enumerate() {
        let word_addr = align_addr + (8u64 * word_idx as u64); // Memory address of word.

        // Write data word to debug module data0/1 registers.
        xpb_explicit_write32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_DATA0,
            vec![(data_word & 0xFFFFFFFF) as u32],
            true,
        );
        xpb_explicit_write32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_DATA1,
            vec![(data_word >> 32) as u32],
            true,
        );

        // Execute abstract command to write data word to RFPC GPR a1.
        if rfpc_dbg_abstractcmd(expl_bar, rfpc, 0, 0x33100b) != 0 {
            panic!("RFPC abstract command returned error.");
        }

        // Write 64-bit word address to debug module data0/1 registers.
        xpb_explicit_write32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_DATA0,
            vec![(word_addr & 0xFFFFFFFF) as u32],
            true,
        );
        xpb_explicit_write32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_DATA1,
            vec![(word_addr >> 32) as u32],
            true,
        );

        // Write instruction to debug module progbuf0 register.
        xpb_explicit_write32(
            expl_bar,
            &rfpc.island,
            rfpc.dm_xpb_base() + RISCV_DBG_PROGBUF0,
            vec![0xB53023],
            true,
        );

        // Execute abstract command to store the double word from a1 into memory at address in a0.
        if rfpc_dbg_abstractcmd(expl_bar, rfpc, 0, 0x37100a) != 0 {
            panic!("RFPC abstract command returned error.");
        }
    }

    // Restore RFPC GPRs a0 and a1.
    rfpc_dbg_write_reg(expl_bar, rfpc, 0x100a, temp_a0);
    rfpc_dbg_write_reg(expl_bar, rfpc, 0x100b, temp_a1);
}
