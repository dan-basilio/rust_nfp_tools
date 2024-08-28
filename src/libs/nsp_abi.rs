#![allow(dead_code)]

use crate::libs::expansion_bar::ExpansionBar;
use crate::libs::cpp_bus::CppIsland;
use bitfield::bitfield;
use bitfield::fmt::Debug;
use std::time::{Duration, Instant};
use std::thread::sleep;

const ABI_LEN_PF: u64 = 128;
const ABI_CTM_BASE_ADDR: u64 = 0x00000000;
const ABI_LOCK_OFFSET: u64 = 0x00000000;
const ABI_CMD_OFFSET: u64 = 0x00000008;
const ABI_RESPONSE_OFFSET: u64 = 0x00000010;


struct LoadFw {
    start: u32,
}

struct ControlRfpcCore {
    option: u32,
    island: u32,
    group: u32,
    core: u32,
}

#[derive(Debug)]
enum AbiMetadataDetails {
    FwLoad(LoadFw),
    RfpcCmd(ControlRfpcCore),
}

// Core structure of ABI metadata fields
#[derive(Debug)]
struct AbiMetadata {
    lock: u64,
    command: u64,
    response: u64,
    details: AbiMetadataDetails,
}

impl AbiMetadata {
    fn new() -> Self {
        Self {
            lock: 0,
            command: 0,
            response: 0,
            details: AbiMetadataDetails::Raw([0; 58]),
        }
    }
}

struct NspAbi {
    cpp_bus: CppBus,
    abi_offset: u64,
    raw_abi_words: Vec<u64>,
}

impl NspAbi {
    pub fn new(pci_bdf: String, exp_bar: &'a mut ExpansionBar) -> Self {
        let pf = pci_bdf.splitn(2, ".").nth(1).unwrap_or("0");
        let pf = u32::from_str_radix(pf, 16).unwrap_or(0);
        let abi_offset = ABI_CTM_BASE_ADDR + pf * ABI_LEN_PF;
        let raw_abi_words: Vec<u64> = Vec::new();

        // Instantiate Cpp bus with allocated expansion BAR.
        let mut cpp_bus = CppBus::new(exp_bar);

        NspAbi { cpp_bus, abi_offset, raw_abi_words }
    }

    pub fn get_lock(&mut self) -> bool {
        // 10-second timeout.
        let timeout = Instant::now() + Duration::from_secs(10);

        loop {
            // Use atomic bitwise-OR immediate command to set the lower byte of
            // the lock word. If the returned value is zero, then the lock was
            // previously unlocked and now held (acquired successfully).
            let lock_val = self.cpp_bus.read(
                CppIsland::ChipExec,
                CppTarget::Mem,
                5,
                3,
                CppLength::Len32,
                self.abi_offset + ABI_LOCK_OFFSET,
                1,
            );

            if lock_val == [0; 4] {
                return true;  // Lock acquired
            }

            // Wait before retry.
            sleep(Duration::from_millis(500));

            // Check if the timeout has been reached.
            if Instant::now() > timeout {
                panic!("Could not acquire NSP ABI lock.");
            }
        }

        false
    }

    pub fn release_lock(&mut self) {
        // Zero the lock word.
        self.cpp_bus.write(
            CppIsland::ChipExec,
            CppTarget::Mem,
            4,
            0,
            CppLength::Len32,
            self.abi_offset + ABI_LOCK_OFFSET,
            vec![0]
        );
    }

    pub fn read_raw_abi(&mut self) {
        let read_words = self.cpp_bus.read(
            CppIsland::ChipExec,
            CppTarget::Mem,
            34,
            0,
            CppLength::Len32,
            self.abi_offset + ABI_LOCK_OFFSET,
            ABI_LEN_PF,
        );
        let qword_slice: &[u64] = cast_slice(&read_words);
        self.raw_abi_words = qword_slice.to_vec();
    }

    pub fn send_cmd(&mut self) {
        // Clear response field.
        self.cpp_bus.write(
            CppIsland::ChipExec,
            CppTarget::Mem,
            4,
            0,
            CppLength::Len32,
            self.abi_offset + ABI_RESPONSE_OFFSET,
            vec![0]
        );



        // Write command details/data in 60-byte chunks.
        let details = &self.data.details;
        let detail_bytes = unsafe {
            slice::from_raw_parts(details.as_ptr() as *const u8, details.len() * 4) // 4 bytes per u32
        };

        for offs in (0..detail_bytes.len()).step_by(60) {
            // Calculate the end of the slice
            let end = std::cmp::min(offs + 60, detail_bytes.len());
            let data = &detail_bytes[offs..end];
            self.expa_bar.write(
                self.bar_offs + ABIMetadata::details_offset() + offs,
                data,
            );
        }

        // Write command itself.
        self.expa_bar.write(
            self.bar_offs + ABIMetadata::command_offset(),
            &self.data.command.to_le_bytes(),
        );
    }

    def send_cmd(self):
        """
        Send command to the NSP on the device.
        """
        # Clear response field.
        self.expa_bar.write(self.bar_offs + ABIMetadata_s.response.offset,
                            words_to_bytes([self.data.fields.response],
                                           word_size_bits=64))

        # Write command details/data.
        details = words_to_bytes(self.data.fields.details.raw, word_size_bits=32)
        for offs in range(0, len(details), 60):
            # Write in 60 byte chunks to avoid data corruption issue.
            data = details[offs:offs + 60]
            self.expa_bar.write(self.bar_offs + ABIMetadata_s.details.offset + offs, data)

        # Write command itself.
        self.expa_bar.write(self.bar_offs + ABIMetadata_s.command.offset,
                            words_to_bytes([self.data.fields.command],
                                           word_size_bits=32))
}

    def get_response(self):
        """
        Read the NSP ABI data from the expansion BAR, and return the
        response code.

        Returns
        -------
        int
            Return/response code from the NSP device.
            0 = result not ready (execution in progress).
            1 = success (command completed successfully).
            any other value indicates failure.
        """
        self.read_abi_bar()
        return self.data.fields.response

    def wait_for_return(self, timeout=300):
        """
        Wait for the current/issued command to be completed, and return
        the resulting return code.

        Parameters
        ----------
        timeout : int, default=300
            Timeout to wait for NSP to respond (in seconds).
            If the timeout is exceeded, an exception is raised.

        Returns
        int
            Return code from the NSP operation performed on the device.
        """
        if timeout is not None:
            timeout_time = time.monotonic() + timeout

        while True:
            rc = self.get_response()
            if rc != 0:
                return rc

            if timeout is not None and time.monotonic() > timeout_time:
                raise TimeoutError("Timeout exceeded waiting for NSP response.")

            time.sleep(0.5)

    def clear_details(self):
        """
        Clear the details fields in the local copy of the ABI data.
        """
        for i in range(len(self.data.fields.details.raw)):
            self.data.fields.details.raw[i] = 0

