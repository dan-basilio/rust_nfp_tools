#![allow(dead_code)]

use crate::libs::exp_bars::ExpansionBar;
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
    WithArg(fn(&mut RspServer<'a>, &str) -> String),
}

pub struct RspServer<'a> {
    //exp_bar: &'a mut ExpansionBar, // Reference to an ExpansionBar
    cmd_resp_map: HashMap<&'static str, Option<FuncType<'a>>>,
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
    /// * `exp_bar` - A mutable reference to an `ExpansionBar` used to handle expansions in the server.
    /// * `elf_data` - A byte slice representing the ELF file data to be parsed.
    ///
    /// # Returns
    ///
    /// Returns a new instance of `RspServer` with the parsed ELF file and a reference
    /// to the provided `ExpansionBar`.
    ///
    /// # Panics
    ///
    /// Panics if the provided ELF data cannot be parsed into a valid ELF file.
    pub fn new() -> Self {
        // Create a Hash map for possible command->function key->value pairs.
        let mut cmd_resp_map: HashMap<&'static str, Option<FuncType>> = HashMap::new();
        cmd_resp_map.insert("!", Some(FuncType::NoArg(RspServer::cmd_not_supported)));
        cmd_resp_map.insert("?", Some(FuncType::Ascii(format!("S{:02x}", 18))));
        cmd_resp_map.insert("c", None);
        cmd_resp_map.insert("D", None);
        cmd_resp_map.insert(
            "QStartNoAckMode",
            Some(FuncType::NoArg(RspServer::toggle_ack)),
        );
        cmd_resp_map.insert("qC", Some(FuncType::Ascii("-1".to_string())));
        cmd_resp_map.insert("qOffsets", Some(FuncType::NoArg(RspServer::load_offsets)));
        cmd_resp_map.insert(
            "qSupported",
            Some(FuncType::WithArg(RspServer::supported_features)),
        );
        cmd_resp_map.insert("qAttached", Some(FuncType::Ascii("1".to_string())));
        cmd_resp_map.insert("H", Some(FuncType::Ascii("l".to_string())));
        cmd_resp_map.insert(
            "g",
            Some(FuncType::Ascii(
                "0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000\
                 0000000000000000"
                    .to_string(),
            )),
        );
        cmd_resp_map.insert("p", Some(FuncType::Ascii("0000000000000000".to_string())));
        cmd_resp_map.insert("P", Some(FuncType::WithArg(RspServer::write_csr)));
        cmd_resp_map.insert("m", None);
        cmd_resp_map.insert("\x03", None);
        cmd_resp_map.insert("k", None);
        cmd_resp_map.insert("C", None);
        cmd_resp_map.insert(
            "vMustReplyEmpty",
            Some(FuncType::NoArg(RspServer::cmd_not_supported)),
        );
        cmd_resp_map.insert("X", Some(FuncType::WithArg(RspServer::load_program)));

        // Server key->value and value support.
        let mut server_v_support: Vec<String> = Vec::new();
        server_v_support.push("qMemoryRead+".to_string());
        let mut server_kv_support: HashMap<String, String> = HashMap::new();
        server_kv_support.insert("PacketSize".to_string(), "100000".to_string());

        // Client key->value and value support.
        let client_kv_support: HashMap<String, String> = HashMap::new();
        let client_v_support: Vec<String> = Vec::new();

        // Keep +/- ACK om until the client disables it
        let disable_ack = false;

        // Return the server struct.
        RspServer {
            // exp_bar,
            cmd_resp_map,
            server_kv_support,
            server_v_support,
            client_kv_support,
            client_v_support,
            disable_ack,
        }
    }

    fn cmd_not_supported(&mut self) -> String {
        "".to_string()
    }

    fn write_csr(&mut self, _packet: &str) -> String {
        "OK".to_string()
    }

    fn load_program(&mut self, _packet: &str) -> String {
        "OK".to_string()
    }

    fn load_offsets(&mut self) -> String {
        "Text=000;Data=000;Bss=000".to_string()
    }

    /// Processes the supported features specified in the input argument and returns a string
    /// containing the key-value pairs of features supported by both the server and the client.
    ///
    /// This method parses the input string to extract features, updating the internal client
    /// key-value support structure. It then constructs a response that includes only the
    /// key-value pairs present in both the client's and the server's supported features.
    /// Additionally, it checks for values supported by both the server and the client and
    /// includes them in the response if they exist.
    ///
    /// # Arguments
    ///
    /// * `arg` - A string argument containing features in the format `prefix: key1=value1;key2=value2;...`.
    ///           The method splits the argument to isolate the features portion.
    ///
    /// # Returns
    ///
    /// A semicolon-separated string of supported key-value pairs and values. If no supported
    /// features are found in common between the server and client, an empty string is returned.
    fn supported_features(&mut self, arg: &str) -> String {
        let args = arg.splitn(2, ':').nth(1).unwrap_or("");

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

    pub fn toggle_ack(&mut self) -> String {
        if !self.disable_ack {
            self.disable_ack = true;
        }
        "OK".to_string()
    }

    /// Handles an incoming RSP packet and provides an appropriate response.
    ///
    /// This function checks the content of the packet and responds based on the command.
    ///
    /// # Parameters
    ///
    /// * `packet` - The received RSP packet as a string.
    ///
    /// # Returns
    ///
    /// A string Option with return value sent back to the GDB client or None (no response)
    fn handle_packet(&mut self, packet: &str) -> Option<String> {
        let rsp_command = packet
            .split(":")
            .next()
            .expect("Iterator could not find split character");

        // First, try to find the full command in the HashMap
        if let Some(response) = self.cmd_resp_map.get(rsp_command) {
            match response {
                Some(FuncType::Ascii(resp)) => return Some(resp.to_string()),
                Some(FuncType::NoArg(func)) => return Some(func(self)),
                Some(FuncType::WithArg(func)) => return Some(func(self, packet)),
                None => return None,
            }
        }

        // If not found, try to find the first character of the command
        // Create a single-character string slice, not a String, for lookup
        let first_char_str = &rsp_command[0..1]; // Slice the first character as a str

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
    /// The checksum is calculated by summing the ASCII byte values of all characters in the data
    /// and taking the result modulo 256. This function is used to verify packet integrity.
    ///
    /// # Parameters
    ///
    /// * `data` - A string slice containing the data for which the checksum will be computed.
    ///
    /// # Returns
    ///
    /// A `u8` value representing the computed checksum.
    fn calculate_rsp_checksum(&self, data: &Vec<u8>) -> u8 {
        data.iter().fold(0, |acc, &b| acc.wrapping_add(b))
    }

    /// Parses an incoming RSP packet from a TCP stream.
    ///
    /// This function reads the raw bytes from the provided `TcpStream` one byte at a time, looking for
    /// the start of an RSP packet (indicated by `$`), then reads the packet contents until it encounters
    /// the end of the packet (indicated by `#`). After reading the packet, the checksum is validated.
    ///
    /// # Parameters
    ///
    /// * `stream` - A mutable reference to the `TcpStream` from which the packet will be read.
    ///
    /// # Returns
    ///
    /// * `Ok(Some(String))` - If a valid packet is successfully parsed and the checksum matches, the packet data is returned.
    /// * `Ok(None)` - If the stream is closed by the client, indicating the end of the connection.
    /// * `Err(std::io::Error)` - If there is a checksum mismatch or another I/O error during packet reading.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the packet cannot be read correctly or if the checksum validation fails.
    fn parse_rsp_packet(&self, stream: &mut TcpStream) -> std::io::Result<String> {
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

        // Read the checksum (two hex characters) after the '#'.
        let mut checksum: [u8; 2] = [0; 2];
        stream.read_exact(&mut checksum)?;
        raw_data.extend_from_slice(&checksum); // Collect checksum

        // Print the entire raw packet (from '$' to '#' inclusive, with checksum).
        // if let Ok(raw_string) = String::from_utf8(raw_data.clone()) {
        //     println!("Raw packet received: {}", raw_string);
        // } else {
        //     println!("Raw packet received (non-UTF8): {:?}", raw_data);
        // }

        // Convert received (and unescaped) data (content between '$' and '#') to string.
        let data = String::from_utf8_lossy(&buffer).to_string();

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
            Ok(data)
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
    /// This function constructs a valid RSP packet by prepending a `$` character,
    /// appending a `#` character, and calculating the checksum.
    ///
    /// # Parameters
    ///
    /// * `response` - The response data to be sent back to the client as a string.
    ///
    /// # Returns
    ///
    /// A `String` representing the formatted RSP packet, ready to be sent over the TCP stream.
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
    /// This function listens for incoming connections on the specified IP and port. Once a connection
    /// is established, it enters a loop where it reads and processes RSP packets from the client.
    /// The server continues to handle packets until instructed to shut down (via the `running` flag)
    /// or if the client disconnects. Each valid packet is processed by the `handle_packet` method.
    ///
    /// # Parameters
    ///
    /// * `running` - An atomic boolean flag (`Arc<AtomicBool>`) indicating whether the server should
    ///   continue running. When this flag is set to `false`, the server will gracefully shut down.
    ///
    /// # Behavior
    ///
    /// * The server binds to a TCP listener on the specified local IP and port.
    /// * It enters a non-blocking mode to avoid stalling while waiting for client connections.
    /// * Once a client connects, the server continuously reads RSP packets in a loop.
    /// * If a valid packet is received, it is processed, and an appropriate response can be sent back.
    /// * The server gracefully shuts down if the `running` flag is set to `false`, if the client
    ///   disconnects, or if an error occurs while reading packets.
    /// * If no connections are available, the server sleeps briefly to avoid busy waiting.
    pub fn run(&mut self, running: Arc<AtomicBool>) {
        // Bind to an address and port.
        let listener =
            TcpListener::bind((LOCAL_HOST_IP, PORT)).expect("Failed to bind to local host!");

        // Set the listener to non-blocking mode
        listener
            .set_nonblocking(true)
            .expect("Cannot set non-blocking");

        println!("Waiting for GDB connection");

        // Main loop: wait for a connection or check if the server should stop
        while running.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, addr)) => {
                    println!("Connected to {:?}", addr);
                    // Handle message from the client
                    while running.load(Ordering::SeqCst) {
                        match self.parse_rsp_packet(&mut stream) {
                            Ok(packet) => {
                                // Handle the packet based on its content
                                match self.handle_packet(&packet) {
                                    Some(resp_data) => {
                                        let resp_send = self.format_rsp_packet(&resp_data);
                                        println!("Reply: {}", resp_send);
                                        stream.write_all(resp_send.as_bytes()).unwrap();
                                    }
                                    None => (), // Do nothing
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
                    // No connection, sleep for a short duration to avoid busy waiting
                    sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    // Unexpected error
                    println!("Error accepting connection: {}", e);
                    break;
                }
            }
        }

        println!("Server shutting down gracefully.");
    }
}
