#![allow(dead_code)]

use crate::libs::cpp_bus::{CppBus, CppIsland, CppLength, CppTarget};
use crate::libs::exp_bars::{ExpansionBar, MapType};

pub fn xpb_read(
    exp_bar: &mut ExpansionBar,
    island: &CppIsland,
    address: u32,
    length: u64,
    xpbm: bool,
) -> Vec<u32> {
    // Ensure expansion BAR gets configured with Bulk mapping.
    if exp_bar.exp_bar_map != MapType::Bulk {
        exp_bar.exp_bar_map = MapType::Bulk;
    }

    let mut xpb_addr = address & 0x00FFFFFF;
    xpb_addr |= (island.id() as u32 & 0x7F) << 24;
    let mut tgt_island = *island;
    if xpbm == true {
        xpb_addr |= 1 << 31; // Set global bit
        tgt_island = CppIsland::ChipExec;
    }

    // Instantiate Cpp bus with allocated expansion BAR.
    let mut cpp_bus = CppBus::new(exp_bar);

    let read_words = cpp_bus.read(
        tgt_island,
        CppTarget::Ct,
        0,
        0,
        CppLength::Len32,
        xpb_addr as u64,
        length,
    );

    read_words
}

pub fn xpb_write(
    exp_bar: &mut ExpansionBar,
    island: &CppIsland,
    address: u32,
    write_words: Vec<u32>,
    xpbm: bool,
) {
    // Ensure expansion BAR gets configured with Bulk mapping.
    if exp_bar.exp_bar_map != MapType::Bulk {
        exp_bar.exp_bar_map = MapType::Bulk;
    }

    let mut xpb_addr = address & 0x00FFFFFF;
    let mut tgt_island = *island;
    if xpbm == true {
        xpb_addr |= 1 << 31; // Set global bit
        tgt_island = CppIsland::ChipExec;
    } else {
        xpb_addr |= (island.id() as u32 & 0x7F) << 24;
    }

    // Instantiate Cpp bus with allocated expansion BAR.
    let mut cpp_bus = CppBus::new(exp_bar);

    cpp_bus.write(
        tgt_island,
        CppTarget::Ct,
        0,
        0,
        CppLength::Len32,
        xpb_addr as u64,
        write_words,
    );
}
