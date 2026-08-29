//! Transport primitives shared by native and platform-provided radio links.

use std::io::{Read, Write};
use std::time::Duration;

use serialport::{ClearBuffer, SerialPort};

/// Blocking byte transport used by Rigwright protocol implementations.
pub trait RadioTransport: Read + Write + Send {
    /// Update the maximum wait for a read operation.
    fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()>;

    /// Discard bytes already waiting in the receive direction.
    ///
    /// Android bulk transports may not have a meaningful equivalent; their
    /// implementation may treat this as a no-op.
    fn clear_input(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    /// Select RTS/CTS hardware flow control when the underlying adapter
    /// supports it. Transports without serial flow-control configuration may
    /// leave this as a no-op.
    fn set_hardware_flow_control(&mut self, _enabled: bool) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) struct SerialPortTransport(pub(crate) Box<dyn SerialPort>);

impl Read for SerialPortTransport {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buffer)
    }
}

impl Write for SerialPortTransport {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl RadioTransport for SerialPortTransport {
    fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()> {
        self.0.set_timeout(timeout).map_err(std::io::Error::other)
    }

    fn clear_input(&mut self) -> std::io::Result<()> {
        self.0
            .clear(ClearBuffer::Input)
            .map_err(std::io::Error::other)
    }

    fn set_hardware_flow_control(&mut self, enabled: bool) -> std::io::Result<()> {
        self.0
            .set_flow_control(if enabled {
                serialport::FlowControl::Hardware
            } else {
                serialport::FlowControl::None
            })
            .map_err(std::io::Error::other)
    }
}

impl<T> RadioTransport for T
where
    T: SerialPort + ?Sized,
{
    fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()> {
        SerialPort::set_timeout(self, timeout).map_err(std::io::Error::other)
    }

    fn clear_input(&mut self) -> std::io::Result<()> {
        SerialPort::clear(self, ClearBuffer::Input).map_err(std::io::Error::other)
    }

    fn set_hardware_flow_control(&mut self, enabled: bool) -> std::io::Result<()> {
        SerialPort::set_flow_control(
            self,
            if enabled {
                serialport::FlowControl::Hardware
            } else {
                serialport::FlowControl::None
            },
        )
        .map_err(std::io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    struct MemoryTransport {
        input: Vec<u8>,
        output: Vec<u8>,
        timeout: Duration,
        flow_control: serialport::FlowControl,
    }

    impl Read for MemoryTransport {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let count = buffer.len().min(self.input.len());
            buffer[..count].copy_from_slice(&self.input[..count]);
            self.input.drain(..count);
            Ok(count)
        }
    }

    impl Write for MemoryTransport {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    impl serialport::SerialPort for MemoryTransport {
        fn name(&self) -> Option<String> { Some("memory".to_string()) }
        fn baud_rate(&self) -> serialport::Result<u32> { Ok(9_600) }
        fn data_bits(&self) -> serialport::Result<serialport::DataBits> { Ok(serialport::DataBits::Eight) }
        fn flow_control(&self) -> serialport::Result<serialport::FlowControl> { Ok(self.flow_control) }
        fn parity(&self) -> serialport::Result<serialport::Parity> { Ok(serialport::Parity::None) }
        fn stop_bits(&self) -> serialport::Result<serialport::StopBits> { Ok(serialport::StopBits::One) }
        fn timeout(&self) -> Duration { self.timeout }
        fn set_baud_rate(&mut self, _baud_rate: u32) -> serialport::Result<()> { Ok(()) }
        fn set_data_bits(&mut self, _data_bits: serialport::DataBits) -> serialport::Result<()> { Ok(()) }
        fn set_flow_control(&mut self, flow_control: serialport::FlowControl) -> serialport::Result<()> {
            self.flow_control = flow_control;
            Ok(())
        }
        fn set_parity(&mut self, _parity: serialport::Parity) -> serialport::Result<()> { Ok(()) }
        fn set_stop_bits(&mut self, _stop_bits: serialport::StopBits) -> serialport::Result<()> { Ok(()) }
        fn set_timeout(&mut self, timeout: Duration) -> serialport::Result<()> {
            self.timeout = timeout;
            Ok(())
        }
        fn write_request_to_send(&mut self, _level: bool) -> serialport::Result<()> { Ok(()) }
        fn write_data_terminal_ready(&mut self, _level: bool) -> serialport::Result<()> { Ok(()) }
        fn read_clear_to_send(&mut self) -> serialport::Result<bool> { Ok(true) }
        fn read_data_set_ready(&mut self) -> serialport::Result<bool> { Ok(true) }
        fn read_ring_indicator(&mut self) -> serialport::Result<bool> { Ok(false) }
        fn read_carrier_detect(&mut self) -> serialport::Result<bool> { Ok(true) }
        fn bytes_to_read(&self) -> serialport::Result<u32> { Ok(self.input.len() as u32) }
        fn bytes_to_write(&self) -> serialport::Result<u32> { Ok(self.output.len() as u32) }
        fn clear(&self, _buffer_to_clear: serialport::ClearBuffer) -> serialport::Result<()> { Ok(()) }
        fn try_clone(&self) -> serialport::Result<Box<dyn serialport::SerialPort>> {
            Err(serialport::Error::new(serialport::ErrorKind::Unknown, "not cloneable"))
        }
        fn set_break(&self) -> serialport::Result<()> { Ok(()) }
        fn clear_break(&self) -> serialport::Result<()> { Ok(()) }
    }

    struct ExternalTransport;

    impl Read for ExternalTransport {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> { Ok(0) }
    }

    impl Write for ExternalTransport {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> { Ok(buffer.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    impl RadioTransport for ExternalTransport {
        fn set_timeout(&mut self, _timeout: Duration) -> std::io::Result<()> { Ok(()) }
    }

    #[test]
    fn radio_transport_defaults_are_noops_for_external_transports() {
        let mut transport = MemoryTransport {
            input: vec![1, 2, 3],
            output: Vec::new(),
            timeout: Duration::from_secs(1),
            flow_control: serialport::FlowControl::None,
        };
        assert_eq!(transport.read(&mut [0; 2]).unwrap(), 2);
        transport.write_all(&[9, 8]).unwrap();
        transport.clear_input().unwrap();
        transport.set_hardware_flow_control(true).unwrap();
        assert_eq!(transport.input, vec![3]);
        assert_eq!(transport.output, vec![9, 8]);

        let mut external = ExternalTransport;
        external.clear_input().unwrap();
        external.set_hardware_flow_control(true).unwrap();
    }

    #[test]
    fn serial_port_transport_delegates_io_and_port_settings() {
        let mut port = MemoryTransport {
            input: vec![4, 5],
            output: Vec::new(),
            timeout: Duration::from_secs(1),
            flow_control: serialport::FlowControl::None,
        };
        assert_eq!(port.name().as_deref(), Some("memory"));
        assert_eq!(port.baud_rate().unwrap(), 9_600);
        assert_eq!(port.data_bits().unwrap(), serialport::DataBits::Eight);
        assert_eq!(port.flow_control().unwrap(), serialport::FlowControl::None);
        assert_eq!(port.parity().unwrap(), serialport::Parity::None);
        assert_eq!(port.stop_bits().unwrap(), serialport::StopBits::One);
        assert_eq!(port.timeout(), Duration::from_secs(1));
        port.set_baud_rate(19_200).unwrap();
        port.set_data_bits(serialport::DataBits::Seven).unwrap();
        port.set_flow_control(serialport::FlowControl::Hardware).unwrap();
        port.set_parity(serialport::Parity::Even).unwrap();
        port.set_stop_bits(serialport::StopBits::Two).unwrap();
        port.write_request_to_send(true).unwrap();
        port.write_data_terminal_ready(true).unwrap();
        assert!(port.read_clear_to_send().unwrap());
        assert!(port.read_data_set_ready().unwrap());
        assert!(!port.read_ring_indicator().unwrap());
        assert!(port.read_carrier_detect().unwrap());
        assert_eq!(port.bytes_to_read().unwrap(), 2);
        assert_eq!(port.bytes_to_write().unwrap(), 0);
        port.clear(serialport::ClearBuffer::All).unwrap();
        assert!(port.try_clone().is_err());
        port.set_break().unwrap();
        port.clear_break().unwrap();
        let mut transport = SerialPortTransport(Box::new(port));
        let mut input = [0; 2];
        assert_eq!(transport.read(&mut input).unwrap(), 2);
        assert_eq!(input, [4, 5]);
        transport.write_all(&[7, 6]).unwrap();
        transport.flush().unwrap();
        transport.set_timeout(Duration::from_millis(250)).unwrap();
        transport.clear_input().unwrap();
        transport.set_hardware_flow_control(true).unwrap();
    }
}
