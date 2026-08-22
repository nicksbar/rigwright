//! DX Lab Suite Commander TCP backend.
//!
//! Commander listens on TCP port 52002 by default and uses length-prefixed
//! XML-like commands, for example `<command:10>CmdGetFreq<parameters:0>`.
//! The wire format and command names follow the implementation used by
//! WSJT-X's DXLab Suite Commander transceiver.

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;

use crate::hal::{Mode, Radio, RadioCapabilities};

const DEFAULT_ADDRESS: &str = "127.0.0.1:52002";
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1_500);

/// A radio controlled through DX Lab Suite Commander.
#[derive(Debug, Clone)]
pub struct DxLabCommanderRadio {
    address: String,
    timeout: Duration,
}

impl DxLabCommanderRadio {
    /// Connect to Commander at `address`, normally `127.0.0.1:52002`.
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Construct a driver using Commander's default local endpoint.
    pub fn localhost() -> Self {
        Self::new(DEFAULT_ADDRESS)
    }

    /// Override the TCP connect/read timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn resolve_address(&self) -> Result<SocketAddr> {
        let address = self.address.trim();
        if address.is_empty() {
            bail!("a DX Lab Commander host:port address is required");
        }
        address
            .to_socket_addrs()
            .with_context(|| format!("invalid DX Lab Commander address: {address}"))?
            .next()
            .with_context(|| {
                format!("DX Lab Commander address resolved to no endpoints: {address}")
            })
    }

    fn transact(&self, command: &str, expect_reply: bool) -> Result<String> {
        let address = self.resolve_address()?;
        let mut stream = TcpStream::connect_timeout(&address, self.timeout)
            .with_context(|| format!("failed to connect to DX Lab Commander at {address}"))?;
        stream
            .set_read_timeout(Some(self.timeout))
            .context("failed to set DX Lab Commander read timeout")?;
        stream
            .set_write_timeout(Some(self.timeout))
            .context("failed to set DX Lab Commander write timeout")?;
        stream
            .write_all(command.as_bytes())
            .context("failed to write DX Lab Commander command")?;
        stream
            .flush()
            .context("failed to flush DX Lab Commander command")?;

        if !expect_reply {
            return Ok(String::new());
        }

        let mut response = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(bytes) => response.extend_from_slice(&buffer[..bytes]),
                Err(error) if error.kind() == std::io::ErrorKind::TimedOut => break,
                Err(error) => return Err(error).context("failed to read DX Lab Commander reply"),
            }
        }
        String::from_utf8(response).context("DX Lab Commander reply was not UTF-8")
    }

    fn command(&self, command: &str) -> Result<()> {
        self.transact(command, false).map(|_| ())
    }

    fn query(&self, command: &str) -> Result<String> {
        self.transact(command, true)
    }
}

#[async_trait]
impl Radio for DxLabCommanderRadio {
    async fn get_frequency_hz(&self) -> Result<u64> {
        let reply = self.query("<command:10>CmdGetFreq<parameters:0>")?;
        let value = response_value(&reply, "CmdFreq")?;
        parse_frequency_khz(value)
    }

    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        let frequency = format_frequency_khz(hz)?;
        let parameter = format!("<xcvrfreq:{}>{frequency}", frequency.len());
        self.command(&format!(
            "<command:10>CmdSetFreq<parameters:{}>{parameter}",
            parameter.len()
        ))
    }

    async fn get_mode(&self) -> Result<Mode> {
        let reply = self.query("<command:11>CmdSendMode<parameters:0>")?;
        decode_commander_mode(response_value(&reply, "CmdMode")?)
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let value = encode_commander_mode(mode);
        let parameter = format!("<1:{}>{value}", value.len());
        self.command(&format!(
            "<command:10>CmdSetMode<parameters:{}>{parameter}",
            parameter.len()
        ))
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        let command = if enabled {
            "<command:5>CmdTX<parameters:0>"
        } else {
            "<command:5>CmdRX<parameters:0>"
        };
        self.command(command)
    }

    fn capabilities(&self) -> RadioCapabilities {
        RadioCapabilities {
            can_get_frequency: true,
            can_set_frequency: true,
            can_get_mode: true,
            can_set_mode: true,
            can_get_ptt: false,
            can_set_ptt: true,
            can_raw_protocol: false,
        }
    }
}

fn response_value<'a>(response: &'a str, name: &str) -> Result<&'a str> {
    let prefix = format!("<{name}:");
    let start = response
        .find(&prefix)
        .with_context(|| format!("DX Lab Commander reply did not contain {name}: {response}"))?;
    let length_start = start + prefix.len();
    let length_end = response[length_start..]
        .find('>')
        .map(|offset| length_start + offset)
        .context("DX Lab Commander reply had malformed length prefix")?;
    let value_start = length_end + 1;
    let length: usize = response[length_start..length_end]
        .parse()
        .context("DX Lab Commander reply had invalid value length")?;
    let value_end = value_start + length;
    response
        .get(value_start..value_end)
        .context("DX Lab Commander reply value was shorter than declared")
}

fn format_frequency_khz(hz: u64) -> Result<String> {
    if hz > 999_999_999_999 {
        bail!("frequency is too large for DX Lab Commander: {hz} Hz");
    }
    Ok(format!("{:>10.3}", hz as f64 / 1_000.0))
}

fn parse_frequency_khz(value: &str) -> Result<u64> {
    let normalized = value.trim().replace(',', "");
    let khz: f64 = normalized
        .parse()
        .with_context(|| format!("invalid DX Lab Commander frequency: {value}"))?;
    if !khz.is_finite() || khz < 0.0 {
        bail!("invalid DX Lab Commander frequency: {value}");
    }
    Ok((khz * 1_000.0).round() as u64)
}

fn encode_commander_mode(mode: Mode) -> &'static str {
    match mode {
        Mode::Lsb => "LSB",
        Mode::Usb => "USB",
        Mode::Cw => "CW",
        Mode::Data => "DATA-U",
        Mode::Am => "AM",
        Mode::Fm => "FM",
        Mode::Wfm => "WFM",
        Mode::Rtty => "RTTY",
        Mode::CwReverse => "CW-R",
        Mode::RttyReverse => "RTTY-R",
    }
}

fn decode_commander_mode(value: &str) -> Result<Mode> {
    match value.trim().to_ascii_uppercase().as_str() {
        "LSB" => Ok(Mode::Lsb),
        "USB" => Ok(Mode::Usb),
        "DATA-U" | "DIGU" | "PKT-R" | "DATA-L" | "DIGL" | "PKT" => Ok(Mode::Data),
        "CW" => Ok(Mode::Cw),
        "CW-R" => Ok(Mode::CwReverse),
        "AM" => Ok(Mode::Am),
        "FM" => Ok(Mode::Fm),
        "WBFM" | "WFM" => Ok(Mode::Wfm),
        "RTTY" => Ok(Mode::Rtty),
        "RTTY-R" => Ok(Mode::RttyReverse),
        other => bail!("unsupported DX Lab Commander mode: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_frequency_command() {
        assert_eq!(format_frequency_khz(14_074_000).unwrap(), " 14074.000");
        let frequency = format_frequency_khz(14_074_000).unwrap();
        let parameter = format!("<xcvrfreq:{}>{frequency}", frequency.len());
        assert_eq!(parameter, "<xcvrfreq:10> 14074.000");
    }

    #[test]
    fn parses_frequency_response() {
        assert_eq!(parse_frequency_khz(" 14074.000").unwrap(), 14_074_000);
        assert_eq!(parse_frequency_khz("14,074.125").unwrap(), 14_074_125);
    }

    #[test]
    fn extracts_length_prefixed_response_value() {
        assert_eq!(response_value("<CmdMode:3>USB", "CmdMode").unwrap(), "USB");
    }

    #[test]
    fn maps_documented_commander_modes() {
        assert_eq!(decode_commander_mode("LSB").unwrap(), Mode::Lsb);
        assert_eq!(decode_commander_mode("USB").unwrap(), Mode::Usb);
        assert_eq!(decode_commander_mode("CW-R").unwrap(), Mode::CwReverse);
        assert_eq!(decode_commander_mode("DATA-L").unwrap(), Mode::Data);
        assert_eq!(decode_commander_mode("WBFM").unwrap(), Mode::Wfm);
    }

    #[test]
    fn default_endpoint_is_local() {
        assert_eq!(DxLabCommanderRadio::localhost().address, DEFAULT_ADDRESS);
    }
}
