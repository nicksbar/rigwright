//! Hamlib `rigctld` TCP backend.
//!
//! `rigctld` exposes a simple line-oriented protocol on TCP (default
//! `127.0.0.1:4532`).  Commands are single ASCII letters terminated by a
//! newline; responses are also newline-terminated.  This module implements a
//! [`Radio`] over that protocol so Rigwright can drive any radio that
//! Hamlib supports without a local serial port.
//!
//! Reference: <https://hamlib.sourceforge.net/html/rigctld.1.html>

use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;

use crate::hal::{Mode, Radio, RadioCapabilities};

const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1_500);

/// A radio controlled through a Hamlib `rigctld` TCP connection.
#[derive(Debug, Clone)]
pub struct RigctldRadio {
    address: String,
    timeout: Duration,
}

impl RigctldRadio {
    /// Connect to `rigctld` at the given address (e.g. `127.0.0.1:4532`).
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Override the default TCP connect/read timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn resolve_address(&self) -> Result<SocketAddr> {
        let address = self.address.trim();
        if address.is_empty() {
            bail!("a rigctld host:port address is required");
        }
        let mut addrs = address
            .to_socket_addrs()
            .with_context(|| format!("invalid rigctld address: {address}"))?;
        addrs
            .next()
            .with_context(|| format!("rigctld address resolved to no endpoints: {address}"))
    }

    fn connect(&self) -> Result<TcpStream> {
        let addr = self.resolve_address()?;
        TcpStream::connect_timeout(&addr, self.timeout)
            .with_context(|| format!("failed to connect to rigctld at {addr}"))
    }

    fn transact(&self, command: &str, expect_response: bool) -> Result<String> {
        let mut stream = self.connect()?;
        stream
            .set_read_timeout(Some(self.timeout))
            .context("failed to set rigctld read timeout")?;
        stream
            .set_write_timeout(Some(self.timeout))
            .context("failed to set rigctld write timeout")?;

        let line = format!("{command}\n");
        stream
            .write_all(line.as_bytes())
            .context("failed to write rigctld command")?;
        stream.flush().context("failed to flush rigctld command")?;

        if !expect_response {
            return Ok(String::new());
        }

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .context("failed to read rigctld response")?;

        // `rigctld` returns `RPRT -<errno>` on errors.  Some versions prefix
        // the value with a `+` for set commands; strip it before parsing.
        let trimmed = response.trim();
        if let Some(rest) = trimmed.strip_prefix("RPRT ") {
            let code: i32 = rest
                .parse()
                .with_context(|| format!("rigctld returned unparsable error: {response}"))?;
            if code != 0 {
                bail!("rigctld command failed with error code {code}");
            }
            return Ok(String::new());
        }

        Ok(trimmed.to_string())
    }

    /// Send a set command and expect either an empty line or `RPRT 0`.
    fn set(&self, command: &str) -> Result<()> {
        self.transact(command, true).map(|_| ())
    }

    /// Send a get command and return the value line.
    fn get(&self, command: &str) -> Result<String> {
        self.transact(command, true)
    }
}

#[async_trait]
impl Radio for RigctldRadio {
    async fn get_frequency_hz(&self) -> Result<u64> {
        let text = self.get("f")?;
        text.parse()
            .with_context(|| format!("invalid rigctld frequency response: {text}"))
    }

    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        self.set(&format!("F {hz}"))
    }

    async fn get_mode(&self) -> Result<Mode> {
        let text = self.get("m")?;
        let mode_token = text.split_whitespace().next().unwrap_or(&text);
        decode_rigctld_mode(mode_token)
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let token = encode_rigctld_mode(mode);
        self.set(&format!("M {token} 0"))
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        self.set(&format!("T {}", if enabled { 1 } else { 0 }))
    }

    fn capabilities(&self) -> RadioCapabilities {
        RadioCapabilities {
            can_get_frequency: true,
            can_set_frequency: true,
            can_get_mode: true,
            can_set_mode: true,
            can_get_ptt: false,
            can_set_ptt: true,
            can_get_power: false,
            can_set_power: false,
            can_raw_protocol: false,
        }
    }
}

fn encode_rigctld_mode(mode: Mode) -> &'static str {
    match mode {
        Mode::Lsb => "LSB",
        Mode::Usb => "USB",
        Mode::Cw => "CW",
        Mode::Data => "PKTUSB",
        Mode::Am => "AM",
        Mode::Fm => "FM",
        Mode::Wfm => "WFM",
        Mode::Rtty => "RTTY",
        Mode::CwReverse => "CWR",
        Mode::RttyReverse => "RTTYR",
    }
}

fn decode_rigctld_mode(token: &str) -> Result<Mode> {
    let normalized = token.to_ascii_uppercase();
    match normalized.as_str() {
        "LSB" => Ok(Mode::Lsb),
        "USB" => Ok(Mode::Usb),
        "PKTUSB" | "PKTLSB" | "DATA-U" | "DATA-L" | "PKTFM" | "DATA-FM" => Ok(Mode::Data),
        "CW" => Ok(Mode::Cw),
        "CWR" => Ok(Mode::CwReverse),
        "FM" => Ok(Mode::Fm),
        "AM" => Ok(Mode::Am),
        "WFM" => Ok(Mode::Wfm),
        "RTTY" => Ok(Mode::Rtty),
        "RTTYR" => Ok(Mode::RttyReverse),
        _ => bail!("unsupported rigctld mode: {token}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::NullRadio;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn encodes_common_modes() {
        assert_eq!(encode_rigctld_mode(Mode::Lsb), "LSB");
        assert_eq!(encode_rigctld_mode(Mode::Usb), "USB");
        assert_eq!(encode_rigctld_mode(Mode::Cw), "CW");
        assert_eq!(encode_rigctld_mode(Mode::Data), "PKTUSB");
    }

    #[test]
    fn decodes_common_modes() {
        assert_eq!(decode_rigctld_mode("LSB").unwrap(), Mode::Lsb);
        assert_eq!(decode_rigctld_mode("USB").unwrap(), Mode::Usb);
        assert_eq!(decode_rigctld_mode("CW").unwrap(), Mode::Cw);
        assert_eq!(decode_rigctld_mode("CWR").unwrap(), Mode::CwReverse);
        assert_eq!(decode_rigctld_mode("PKTUSB").unwrap(), Mode::Data);
        assert_eq!(decode_rigctld_mode("DATA-U").unwrap(), Mode::Data);
        assert_eq!(decode_rigctld_mode("FM").unwrap(), Mode::Fm);
        assert_eq!(decode_rigctld_mode("AM").unwrap(), Mode::Am);
    }

    #[test]
    fn rejects_unknown_mode() {
        assert!(decode_rigctld_mode("UNKNOWN").is_err());
    }

    #[test]
    fn empty_address_is_rejected() {
        let radio = RigctldRadio::new("");
        assert!(radio.resolve_address().is_err());
    }

    #[test]
    fn invalid_address_is_rejected() {
        let radio = RigctldRadio::new("not-a-valid-address");
        assert!(radio.resolve_address().is_err());
    }

    #[test]
    fn default_address_resolves_to_localhost() {
        let radio = RigctldRadio::new("127.0.0.1:4532");
        let addr = radio.resolve_address().unwrap();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert_eq!(addr.port(), 4532);
    }

    #[test]
    fn null_radio_stores_frequency_and_mode() {
        let radio = NullRadio::with_frequency_mode(7_074_000, Mode::Lsb);
        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            7_074_000
        );
        assert_eq!(
            futures::executor::block_on(radio.get_mode()).unwrap(),
            Mode::Lsb
        );
    }

    #[test]
    fn null_radio_updates_state() {
        let radio = NullRadio::new();
        futures::executor::block_on(radio.set_frequency_hz(21_250_000)).unwrap();
        futures::executor::block_on(radio.set_mode(Mode::Cw)).unwrap();
        futures::executor::block_on(radio.set_ptt(true)).unwrap();
        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            21_250_000
        );
        assert_eq!(
            futures::executor::block_on(radio.get_mode()).unwrap(),
            Mode::Cw
        );
        assert!(radio.capabilities().can_get_ptt);
    }

    #[test]
    fn loopback_server_exercises_rigctld_command_and_error_paths() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&commands);
        let server = thread::spawn(move || {
            for index in 0..6 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                seen.lock().unwrap().push(line.trim().to_string());
                let response = match index {
                    0 => "14074000\n",
                    1 => "RPRT 0\n",
                    2 => "USB 0\n",
                    3 | 4 => "RPRT 0\n",
                    _ => "RPRT -1\n",
                };
                stream.write_all(response.as_bytes()).unwrap();
            }
        });

        let radio = RigctldRadio::new(address.to_string()).with_timeout(Duration::from_secs(1));
        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_074_000
        );
        futures::executor::block_on(radio.set_frequency_hz(14_075_000)).unwrap();
        assert_eq!(
            futures::executor::block_on(radio.get_mode()).unwrap(),
            Mode::Usb
        );
        futures::executor::block_on(radio.set_mode(Mode::Data)).unwrap();
        futures::executor::block_on(radio.set_ptt(true)).unwrap();
        assert!(futures::executor::block_on(radio.set_ptt(false)).is_err());
        server.join().unwrap();
        assert_eq!(
            commands.lock().unwrap().as_slice(),
            ["f", "F 14075000", "m", "M PKTUSB 0", "T 1", "T 0"]
        );
        assert!(radio.capabilities().can_set_ptt);
        assert!(!radio.capabilities().can_get_ptt);
    }
}
