#![allow(dead_code)]

use crate::libs::cpp_bus::CppIsland;
use crate::libs::expansion_bar::ExpansionBar;
use crate::libs::explicit_bar::ExplicitBar;
use crate::libs::mem_access::{mem_write, MemoryType, MuMemoryEngine};
use crate::libs::rfpc::{Rfpc, RfpcCsr, RfpcGpr, RfpcReg};
use crate::libs::rfpc_debugger::{rfpc_dbg_read_reg, rfpc_dbg_resume, rfpc_dbg_write_reg};
use bytemuck::cast_slice;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

const LOCAL_HOST_IP: &str = "127.0.0.1";
const PORT: u16 = 12727;

// Define the function type enum.
#[derive(Clone)]
enum FuncType<'a> {
    Ascii(String),
    NoArg(fn(&mut RspServer<'a>) -> String),
    WithArg(fn(&mut RspServer<'a>, Vec<u8>) -> String),
}

pub struct RspServer<'a> {
    exp_bar: &'a mut ExpansionBar,
    expl_bar: &'a mut ExplicitBar,
    cmd_resp_map: HashMap<String, Option<FuncType<'a>>>,
    server_kv_support: HashMap<String, String>,
    server_v_support: Vec<String>,
    client_kv_support: HashMap<String, String>,
    client_v_support: Vec<String>,
    disable_ack: bool,
}

impl<'a> RspServer<'a> {
    /// Creates a new instance of the `RspServer`.
    ///
    /// # Parameters
    ///
    /// * `exp_bar: &'a mut ExpansionBar` - A mutable reference to an
    ///   `ExpansionBar`.
    ///
    /// # Returns
    ///
    /// `RspServer` instance.
    pub fn new(exp_bar: &'a mut ExpansionBar, expl_bar: &'a mut ExplicitBar) -> Self {
        let mut cmd_resp_map: HashMap<String, Option<FuncType>> = HashMap::new();
        cmd_resp_map.insert(
            "!".to_string(),
            Some(FuncType::NoArg(RspServer::cmd_not_supported)),
        );
        cmd_resp_map.insert(
            "?".to_string(),
            Some(FuncType::Ascii(format!("S{:02x}", 18))),
        );
        cmd_resp_map.insert("c".to_string(), None);
        cmd_resp_map.insert("D".to_string(), None);
        cmd_resp_map.insert(
            "QStartNoAckMode".to_string(),
            Some(FuncType::NoArg(RspServer::toggle_ack)),
        );
        cmd_resp_map.insert("qC".to_string(), Some(FuncType::Ascii("-1".to_string())));
        cmd_resp_map.insert(
            "qOffsets".to_string(),
            Some(FuncType::NoArg(RspServer::load_offsets)),
        );
        cmd_resp_map.insert(
            "qSupported".to_string(),
            Some(FuncType::WithArg(RspServer::supported_features)),
        );
        cmd_resp_map.insert(
            "qAttached".to_string(),
            Some(FuncType::Ascii("1".to_string())),
        );
        cmd_resp_map.insert("H".to_string(), Some(FuncType::Ascii("l".to_string())));
        cmd_resp_map.insert("g".to_string(), Some(FuncType::NoArg(RspServer::read_gprs)));
        cmd_resp_map.insert(
            "p".to_string(),
            Some(FuncType::Ascii("0000000000000000".to_string())),
        );
        cmd_resp_map.insert(
            "P".to_string(),
            Some(FuncType::WithArg(RspServer::write_csr)),
        );
        cmd_resp_map.insert("m".to_string(), None);
        cmd_resp_map.insert("\x03".to_string(), None);
        cmd_resp_map.insert("k".to_string(), None);
        cmd_resp_map.insert("C".to_string(), None);
        cmd_resp_map.insert(
            "vMustReplyEmpty".to_string(),
            Some(FuncType::NoArg(RspServer::cmd_not_supported)),
        );
        cmd_resp_map.insert(
            "X".to_string(),
            Some(FuncType::WithArg(RspServer::load_segment)),
        );

        // Server key->value and value support.
        let mut server_v_support: Vec<String> = Vec::new();
        server_v_support.push("qMemoryRead+".to_string());
        server_v_support.push("swbreak+".to_string());
        let mut server_kv_support: HashMap<String, String> = HashMap::new();
        server_kv_support.insert("PacketSize".to_string(), "100000".to_string());

        // Client key->value and value support.
        let client_kv_support: HashMap<String, String> = HashMap::new();
        let client_v_support: Vec<String> = Vec::new();

        // Keep +/- ACK om until the client disables it
        let disable_ack = false;

        // Return the server struct.
        RspServer {
            exp_bar,
            expl_bar,
            cmd_resp_map,
            server_kv_support,
            server_v_support,
            client_kv_support,
            client_v_support,
            disable_ack,
        }
    }

    /// Method that returns an empty string if the RSP command is not
    /// supported.
    ///
    /// # Returns
    ///
    /// `String` - Empty string.
    fn cmd_not_supported(&mut self) -> String {
        "".to_string()
    }

    /// Returns a concatenated string of the all the GPR register values
    /// in hex.
    ///
    /// # Returns
    ///
    /// `String` - Concatenated list of GPR values.
    fn read_gprs(&mut self) -> String {
        let mut gprs = String::new();
        let rfpc = Rfpc {
            island: CppIsland::Rfpc0,
            cluster: 0,
            group: 0,
            core: 0,
        };

        // Read all the GPRs and send them to the debug client.
        let mut reg_addr = RfpcGpr::X0.reg_addr();
        let mut reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X1.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X2.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X3.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X4.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X5.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X6.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X7.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X8.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X9.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X10.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X11.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X12.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X13.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X14.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X15.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X16.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X17.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X18.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X19.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X20.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X21.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X22.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X23.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X24.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X25.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X26.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X27.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X28.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X29.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X30.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        reg_addr = RfpcGpr::X31.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        // Read the program counter and send to the debug client.
        reg_addr = RfpcCsr::Dpc.reg_addr();
        reg_val = rfpc_dbg_read_reg(self.expl_bar, &rfpc, reg_addr);
        gprs.push_str(&format!("{:016x}", reg_val.swap_bytes()));

        gprs
    }

    fn write_csr(&mut self, packet: Vec<u8>) -> String {
        // Create RFPC instance.
        let rfpc = Rfpc {
            island: CppIsland::Rfpc0,
            cluster: 0,
            group: 0,
            core: 0,
        };

        // Find the position of the =.
        let equals_index = packet.iter().position(|&b| b == b'=').unwrap();

        // Extract the first part (as string), split by the equals.
        let reg_addr = String::from_utf8_lossy(&packet[1..equals_index]);
        let reg_val = String::from_utf8_lossy(&packet[equals_index + 1..]);
        let address = u8::from_str_radix(&reg_addr, 16).expect("Failed to parse address as u8");
        let value = u64::from_str_radix(&reg_val, 16).expect("Failed to parse value as u64");

        if address == 32 {
            let dpc = RfpcCsr::Dpc.reg_addr();
            rfpc_dbg_write_reg(self.expl_bar, &rfpc, dpc, value.swap_bytes());
        }

        "OK".to_string()
    }

    /// Load a program segment into the appropriate memory that is
    /// identified by the programm address. This function facilitates loading
    /// program memory from a GDB client segment by segment.
    ///
    /// # Parameters
    ///
    /// * `packet: Vec<u8>` - RSP packet after being parsed.
    ///
    /// # Returns
    ///
    /// * `String` - Returns OK on successful memory write.
    fn load_segment(&mut self, packet: Vec<u8>) -> String {
        // Find the position of the colon.
        let colon_index = packet.iter().position(|&b| b == b':').unwrap();

        // Extract the first part (as string), split by the comma.
        let buffer_info = String::from_utf8_lossy(&packet[1..colon_index]);
        let mut split_iter = buffer_info.splitn(2, ",");

        // Extract the address.
        let address = split_iter.next().unwrap();

        // Extract the length.
        let length = split_iter.next().unwrap();

        // Convert the address and length.
        let mut address =
            u64::from_str_radix(&address, 16).expect("Failed to parse address as u64");
        let length = u64::from_str_radix(&length, 16).expect("Failed to parse address as u64");

        // The first loaded segment will always be a length of zero and should return OK.
        if length == 0 {
            return "OK".to_string();
        }

        println!("address: {:x}", &address);
        println!("length: {}", &length);

        // Extract target address.
        address &= 0x00000000FFFFFFFF;

        // Extract the program bytes (segment after the colon).
        let mut packet_data = packet[colon_index + 1..].to_vec();

        // Append zero bytes to make the length a multiple of 4 bytes.
        let remainder = length as usize % 4;
        if remainder != 0 {
            packet_data.extend(vec![0; 4 - remainder]);
            panic!("Program segment not a multiple of u32");
        }

        // Cast the byte slice to u32 vec safely.
        let program_data: Vec<u32> = cast_slice(&packet_data).to_vec();

        // Write program segment to memory.
        mem_write(
            self.exp_bar,
            CppIsland::Rfpc0,
            MemoryType::Ctm,
            MuMemoryEngine::Bulk32,
            address,
            program_data,
        );

        "OK".to_string()
    }

    /// Code is not being relocated because the ELF file is assumed to be
    /// statically linked. Therefore the offsets in the address are the offsets
    /// we use on the chip.
    fn load_offsets(&mut self) -> String {
        "Text=000;Data=000;Bss=000".to_string()
    }

    /// This method parses the input packet to extract the features that
    /// the client supports. It then returns the features that the
    /// server supports.
    ///
    /// # Parameters
    ///
    /// * `packet: Vec<u8>` - The parsed packet received from the
    ///   RSP client.
    ///
    /// # Returns
    ///
    /// A semicolon-separated string of supported values and key-value
    /// pairs.
    fn supported_features(&mut self, packet: Vec<u8>) -> String {
        let colon_index = packet.iter().position(|&b| b == b':').unwrap();
        let args = String::from_utf8_lossy(&packet[colon_index + 1..]).to_string();

        // Process each feature in the args
        for feat in args.split(';') {
            if let Some((k, v)) = feat.split_once('=') {
                self.client_kv_support.insert(k.to_string(), v.to_string());
            } else {
                self.client_v_support.push(feat.to_string());
            }
        }

        // Create a response that includes only the key-value pairs from the server
        let mut response: Vec<String> = self
            .server_kv_support
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        // Also add server-supported values
        response.extend(
            self.server_v_support
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>(),
        );

        // Return the joined response
        response.join(";")
    }

    /// Disable packet +/- ACK NACK.
    ///
    /// The GDB client can request that, after connection, packet ACK
    /// and NACK is disabled and no longer necessary.
    ///
    /// # Returns
    ///
    /// A String confirming operation completed successfully.
    pub fn toggle_ack(&mut self) -> String {
        if !self.disable_ack {
            self.disable_ack = true;
        }
        "OK".to_string()
    }

    /// Handles an incoming RSP packet and determines what type of
    /// function to call.
    ///
    /// # Parameters
    ///
    /// * `packet: Vec<u8>` - The parsed RSP packet as a Vec<u8>.
    ///
    /// # Returns
    ///
    /// `Option<String>` - A String Option with return value sent back
    /// to the GDB client.
    fn handle_packet(&mut self, packet: Vec<u8>) -> Option<String> {
        // Extract the command
        let colon_index = packet
            .iter()
            .position(|&b| b == b':')
            .unwrap_or_else(|| packet.len());
        let rsp_command = String::from_utf8_lossy(&packet[..colon_index]).to_string();

        // First, try to find the full command in the HashMap
        if let Some(response) = self.cmd_resp_map.get(&rsp_command) {
            match response {
                Some(FuncType::Ascii(resp)) => return Some(resp.to_string()),
                Some(FuncType::NoArg(func)) => return Some(func(self)),
                Some(FuncType::WithArg(func)) => return Some(func(self, packet)),
                None => return None,
            }
        }

        // If not found, try to find the first character of the command
        // Create a single-character string slice, not a String, for lookup
        let first_char_str = &rsp_command[0..1].to_string();

        if let Some(response) = self.cmd_resp_map.get(first_char_str) {
            match response {
                Some(FuncType::Ascii(resp)) => return Some(resp.to_string()),
                Some(FuncType::NoArg(func)) => return Some(func(self)),
                Some(FuncType::WithArg(func)) => return Some(func(self, packet)),
                None => return None,
            }
        }

        // If neither the command nor the first character is found
        println!("Unknown RSP command {}", rsp_command);
        Some(self.cmd_not_supported())
    }

    /// Calculates the checksum for an RSP packet.
    ///
    /// The checksum is calculated by summing the ASCII byte values of
    /// all characters in the data and taking the result modulo 256.
    /// This function is used to verify packet integrity.
    ///
    /// # Parameters
    ///
    /// * `data: &Vec<u8>` - A vector reference with the data.
    ///
    /// # Returns
    ///
    /// `u8` - value representing the computed checksum.
    fn calculate_rsp_checksum(&self, data: &Vec<u8>) -> u8 {
        data.iter().fold(0, |acc, &b| acc.wrapping_add(b))
    }

    /// Parses an incoming RSP packet from a TCP stream.
    ///
    /// This function reads the raw bytes from the provided `TcpStream`
    /// one byte at a time, looking for the start of an RSP packet
    /// (indicated by `$`), then reads the packet contents until it
    /// encounters the end of the packet (indicated by `#`). After
    /// reading the packet, the checksum is validated.
    ///
    /// # Parameters
    ///
    /// * `stream: &mut TcpStream` - Mutable reference to the `TcpStream`.
    ///
    /// # Returns
    ///
    /// * `Ok(Vec<u8>)` - If there are no errors during packet parsing.
    /// * `Ok(None)` - If the stream is closed by the client.
    /// * `Err(std::io::Error)` - IO error during packet reading.
    fn parse_rsp_packet(&self, stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
        let mut buffer_orig: Vec<u8> = Vec::new();
        let mut buffer: Vec<u8> = Vec::new();
        let mut byte: [u8; 1] = [0; 1];

        // Temporarily collect the entire raw packet for printing, including '$', data, and '#'.
        let mut raw_data: Vec<u8> = Vec::new();

        // Read 1 byte at a time until we find a starting '$'.
        while stream.read(&mut byte)? > 0 {
            if byte[0] == b'$' {
                raw_data.push(byte[0]); // Start collecting from '$'
                break;
            }
        }

        // Read the rest of the packet until we hit '#', handling escaped characters.
        let mut escaped = false;
        while stream.read(&mut byte)? > 0 && byte[0] != b'#' {
            raw_data.push(byte[0]); // Collect packet data for logging purposes
            buffer_orig.push(byte[0]);

            if escaped {
                // Undo the escaping by XORing the byte with 0x20, and add the result to buffer
                buffer.push(byte[0] ^ 0x20);
                escaped = false;
            } else if byte[0] == 0x7d {
                // Escape detected, set the flag and skip adding this byte to buffer
                escaped = true;
            } else {
                // Normal byte, just push it to the buffer
                buffer.push(byte[0]);
            }
        }

        // Now we explicitly read the '#' character to consume it from the stream.
        if byte[0] == b'#' {
            raw_data.push(byte[0]); // Add '#' to raw data
        }

        println!("Raw packet: {:?}", raw_data);

        // Read the checksum (two hex characters) after the '#'.
        let mut checksum: [u8; 2] = [0; 2];
        stream.read_exact(&mut checksum)?;
        raw_data.extend_from_slice(&checksum); // Collect checksum

        // Calculate checksum and validate.
        let expected_checksum = self.calculate_rsp_checksum(&buffer_orig);
        let received_checksum =
            u8::from_str_radix(&String::from_utf8_lossy(&checksum), 16).unwrap_or(0);

        println!(
            "expected_checksum = {}, received_checksum = {}",
            expected_checksum, received_checksum
        );

        if expected_checksum == received_checksum {
            if !self.disable_ack {
                stream.write_all(b"+")?; // Acknowledge valid packet
            }
            Ok(buffer)
        } else {
            // Return an error for checksum mismatch.
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Checksum mismatch",
            ))
        }
    }

    /// Formats a response string into an RSP packet.
    ///
    /// This function constructs a valid RSP packet by prepending a
    /// `$` character, appending a `#` character, and calculating the
    /// checksum.
    ///
    /// # Parameters
    ///
    /// * `response: &str` - The response data to be sent back to the client.
    ///
    /// # Returns
    ///
    /// `String` - A string representing the formatted RSP packet.
    fn format_rsp_packet(&self, response: &str) -> String {
        // Prepend the response with the start character '$'
        let mut packet = format!("${}", response);

        // Append the end character '#'
        packet.push('#');

        // Calculate the checksum
        let checksum = self.calculate_rsp_checksum(&response.as_bytes().to_vec());

        // Append the checksum in hexadecimal format (2 digits)
        packet.push_str(&format!("{:02x}", checksum));

        packet
    }

    /// Runs the RSP server, accepting and handling client connections.
    ///
    /// # Parameters
    ///
    /// * `running : Arc<AtomicBool>` - An atomic boolean flag
    ///   indicating whether the server should continue running. When
    ///   this flag is set to `false`, the server will gracefully shut
    ///   down.
    pub fn run(&mut self, running: Arc<AtomicBool>) {
        // Bind to an address and port.
        let listener =
            TcpListener::bind((LOCAL_HOST_IP, PORT)).expect("Failed to bind to local host!");

        // Set the listener to non-blocking mode.
        listener
            .set_nonblocking(true)
            .expect("Cannot set non-blocking");

        println!("Waiting for GDB connection");

        // Main loop: wait for a connection or check if the server should stop.
        while running.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, addr)) => {
                    println!("Connected to {:?}", addr);
                    // Handle message from the client.
                    while running.load(Ordering::SeqCst) {
                        match self.parse_rsp_packet(&mut stream) {
                            Ok(packet) => {
                                // Handle the packet based on its content.
                                match self.handle_packet(packet) {
                                    Some(resp_data) => {
                                        let resp_send = self.format_rsp_packet(&resp_data);
                                        println!("Reply: {}", resp_send);
                                        stream.write_all(resp_send.as_bytes()).unwrap();
                                    }
                                    None => (), // Do nothing.
                                };
                            }
                            Err(e) => {
                                if !self.disable_ack {
                                    stream.write_all(b"-").unwrap();
                                }
                                println!("Failed to read packet: {}", e);
                            }
                        }
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No connection, sleep for a short duration to avoid busy waiting.
                    sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    // Unexpected error.
                    println!("Error accepting connection: {}", e);
                    break;
                }
            }
        }

        println!("Server shutting down gracefully.");
    }
}
