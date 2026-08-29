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
