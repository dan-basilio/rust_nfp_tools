#![allow(dead_code)]

use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::libs::cpp_bus::CppIsland;
use crate::libs::expansion_bar::ExpansionBar;
use crate::libs::mem_access::{mem_read, mem_write, MemoryType, MuMemoryEngine};
use crate::libs::rfpc::Rfpc;
use bitfield::bitfield;
use bytemuck::cast_slice;
use regex::Regex;

/// Virtual terminal memory offsets (in bytes).
const LOCK_OFFSET: u32 = 0;
const METADATA_OFFSET: u32 = 4;
const LENGTH_OFFSET: u32 = 8;
const DATA_OFFSET: u32 = 12;

bitfield! {
    pub struct VtmMetadata(u32);
    impl Debug;
    u32;
    pub core, set_core: 2, 0;
    pub group, set_group: 7, 3;
    pub island, set_island: 15, 8;
    pub reserved, set_reserved: 29, 16;
    pub direction, set_direction: 30;
    pub manual_lock, set_manual_lock: 31;
}

/// NFP / RFPC virtual terminal instance.
///
/// Represents a single instance of the virtual terminal interface on
/// a specified NFP device, at a specified location in memory.
pub struct VirtualTerminal<'a> {
    exp_bar: &'a mut ExpansionBar,
    island: CppIsland,
    mem_type: MemoryType,
    address: u32,
}

impl<'a> VirtualTerminal<'a> {
    /// Creates a new `VirtualTerminal` instance.
    ///
    /// # Parameters
    ///
    /// - `exp_bar`: `&mut ExpansionBar`
    ///   A mutable reference to the `ExpansionBar` instance, which provides access to the expansion memory region.
    ///
    /// - `island`: `CppIsland`
    ///   The island in which the virtual terminal resides. Determines the memory type used (`Emem` or `Ctm`).
    ///
    /// - `address`: `u32`
    ///   The base address of the virtual terminal memory region in the selected island.
    ///
    /// # Returns
    ///
    /// A new `VirtualTerminal` instance configured with the specified `ExpansionBar`, `CppIsland`, and memory address.
    pub fn new(exp_bar: &'a mut ExpansionBar, island: CppIsland, address: u32) -> Self {
        let mem_type = if island == CppIsland::Emu0 {
            MemoryType::Emem
        } else {
            MemoryType::Ctm
        };

        VirtualTerminal {
            exp_bar,
            island,
            mem_type,
            address,
        }
    }

    /// Return whether the virtual terminal lock is held by an RFPC.
    pub fn is_locked(&mut self) -> bool {
        let lock_word = mem_read(
            self.exp_bar,
            self.island,
            self.mem_type,
            MuMemoryEngine::Atomic32,
            (self.address + LOCK_OFFSET).into(),
            1,
        )[0];

        lock_word == 0
    }

    /// Return the identifying information of the locking RFPC.
    pub fn holder(&mut self) -> Option<Rfpc> {
        // If the virtual terminal is not locked, then the holder is
        // invalid regardless of data presence.
        if !self.is_locked() {
            return None;
        }

        let meta_word = mem_read(
            self.exp_bar,
            self.island,
            self.mem_type,
            MuMemoryEngine::Atomic32,
            (self.address + METADATA_OFFSET).into(),
            1,
        )[0];

        let meta = VtmMetadata(meta_word);

        // Check for invalid island ID. Use a bitwise AND to check the condition.
        if meta.island() == 0 || (meta.island() & 0x300) != 0 {
            // Island ID is invalid, indicating that the metadata is
            // invalid. This typically occurs on startup before the lock
            // is acquired for the first time.
            return None;
        }

        let group = meta.group() % 4;
        let cluster = group / 4;

        Some(Rfpc {
            island: CppIsland::from_id(meta.island() as u8),
            cluster: cluster as u8,
            group: group as u8,
            core: meta.core() as u8,
        })
    }

    /// Return the number of bytes of data available in the virtual
    /// terminal memory.
    pub fn data_available(&mut self) -> u32 {
        match self.holder() {
            Some(_) => {
                // Read the length of available data from the specified offset.
                mem_read(
                    self.exp_bar,
                    self.island,
                    self.mem_type,
                    MuMemoryEngine::Atomic32,
                    (self.address + LENGTH_OFFSET).into(),
                    1,
                )[0]
            }
            None => 0, // If no holder, return 0.
        }
    }

    /// Read the available data from the virtual terminal as bytes.
    pub fn read_bytes(&mut self) -> Vec<u8> {
        let mut data_bytes: Vec<u8> = Vec::new();
        // Get length of available data.
        let data_len = self.data_available();
        if data_len == 0 {
            return data_bytes;
        }

        // Read available data from memory.
        let data_words = mem_read(
            self.exp_bar,
            self.island,
            self.mem_type,
            MuMemoryEngine::Bulk32,
            (self.address + DATA_OFFSET).into(),
            data_len as u64,
        );

        // Clear the length word to indicate to the sender that the data
        // has been received, and it's clear to send more data.
        mem_write(
            self.exp_bar,
            self.island,
            self.mem_type,
            MuMemoryEngine::Atomic32,
            (self.address + LENGTH_OFFSET).into(),
            vec![0],
        );

        let converted_bytes: Vec<u8> = cast_slice(&data_words).to_vec();
        data_bytes.extend(converted_bytes);

        data_bytes
    }

    /// Read the available data from the virtual terminal as a string.
    pub fn read_string(&mut self) -> String {
        String::from_utf8(self.read_bytes()).expect("Invalid UTF-8 sequence")
    }

    /// Waits for data to become available or until a specified timeout is reached.
    ///
    /// This function continuously checks if data is available using the `data_available`
    /// method. If data becomes available, the function exits. If a timeout is provided,
    /// the function will panic with an error message if the time limit is exceeded before
    /// data is available.
    ///
    /// # Arguments
    ///
    /// * `timeout` - An optional duration in seconds to wait for data. If `None`, the function
    ///   will wait indefinitely.
    ///
    /// # Panics
    ///
    /// This function will panic if the timeout is exceeded while waiting for data.
    pub fn wait_for_data(&mut self, timeout: Option<u64>) {
        let end_time = match timeout {
            Some(t) => Some(Instant::now() + Duration::from_secs(t)),
            None => None,
        };

        loop {
            if self.data_available() != 0 {
                break;
            }

            if let Some(end_time) = end_time {
                if Instant::now() > end_time {
                    panic!("Time limit exceeded waiting for virtual terminal data.");
                }
            }

            // Sleep for a short duration before checking again.
            sleep(Duration::from_millis(100));
        }
    }

    /// Reads a block of data, waiting for it to become available with specified timeouts.
    ///
    /// This function continuously reads data and appends it to a buffer until a matching
    /// substring is found based on the provided regular expression or until there is no more
    /// data available. The function can wait for data to be available with a specified
    /// start timeout, and it can also enforce an end timeout between subsequent reads.
    ///
    /// # Arguments
    ///
    /// * `start_timeout` - An optional duration in seconds to wait for data availability
    ///   before starting to read. If `None`, it waits indefinitely.
    /// * `end_timeout` - An optional duration in seconds to wait between reads before timing out.
    ///   If `None`, it checks for data availability continuously without delay.
    /// * `regexp` - A regular expression string to match the read data against. The first match
    ///   found will be returned.
    ///
    /// # Returns
    ///
    /// A `String` containing the first block of data that matches the regular expression.
    /// If no match is found and there is no more data available, it returns the data received so far.
    ///
    /// # Panics
    ///
    /// This function will panic if the regular expression cannot be compiled.
    pub fn read_block(
        &mut self,
        start_timeout: Option<u64>,
        end_timeout: Option<u64>,
        regexp: &str,
    ) -> String {
        if let Some(timeout) = start_timeout {
            // Wait for data to be available (or timeout exceeded).
            self.wait_for_data(Some(timeout));
        }

        let mut block = String::new();
        let regex = Regex::new(regexp).unwrap();

        loop {
            if self.data_available() == 0 {
                // No more data available - return what's received so far.
                return block;
            }

            block.push_str(&self.read_string());

            // Check for regex match
            if let Some(captures) = regex.captures(&block) {
                // Return the full match (group 0)
                return captures[0].to_string();
            }

            if let Some(timeout) = end_timeout {
                self.wait_for_data(Some(timeout));
            } else {
                sleep(Duration::from_millis(100));
            }
        }
    }

    /// Discard the current pending data in the virtual terminal.
    /// Set the `length` word to zero to indicate to the sender that the
    /// data has been processed/consumed, and it's free to send more
    /// data (overwrite the current data), but don't actually read the
    /// pending data.
    pub fn flush_one(&mut self) {
        mem_write(
            self.exp_bar,
            self.island,
            self.mem_type,
            MuMemoryEngine::Atomic32,
            (self.address + LENGTH_OFFSET).into(),
            vec![0],
        );
    }

    /// Flushes all pending data from the virtual terminal interface,
    /// up to a specified timeout. This is done by repeatedly discarding
    /// any pending data until no more data is written or the specified
    /// timeout is reached.
    ///
    /// # Arguments
    ///
    /// * `timeout` - An optional duration in seconds to flush pending data.
    ///   If `None`, the virtual terminal will be continually flushed
    ///   until no more data is pushed to it.
    ///
    /// # Panics
    ///
    /// This function will panic if the timeout is exceeded while flushing data.
    pub fn flush(&mut self, timeout: Option<u64>) {
        let mut end_time: Option<Instant> = None;

        if let Some(timeout) = timeout {
            end_time = Some(Instant::now() + Duration::from_secs(timeout));
        }

        while self.data_available() != 0 {
            if let Some(end_time) = end_time {
                if Instant::now() > end_time {
                    panic!("Time limit exceeded while flushing data from virtual terminal interface. Data still pending in memory.");
                }
            }

            self.flush_one();
            sleep(Duration::from_millis(100));
        }
    }
}
