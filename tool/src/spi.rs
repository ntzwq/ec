use crate::{
    Error,
    Timeout,
};

pub trait Spi {
    fn target(&self) -> SpiTarget;
    unsafe fn reset(&mut self) -> Result<(), Error>;
    unsafe fn read(&mut self, data: &mut [u8]) -> Result<usize, Error>;
    unsafe fn write(&mut self, data: &[u8]) -> Result<usize, Error>;
}

#[derive(Clone, Copy)]
pub enum SpiTarget {
    Main,
    Backup,
}

pub struct SpiRom<'a, S: Spi, T: Timeout> {
    spi: &'a mut S,
    timeout: T,
}

impl<'a, S: Spi, T: Timeout> SpiRom<'a, S, T> {
    pub fn new(spi: &'a mut S, timeout: T) -> Self {
        Self {
            spi,
            timeout,
        }
    }

    pub fn sector_size(&self) -> usize {
        match self.spi.target() {
            SpiTarget::Main => 1024,
            SpiTarget::Backup => 4096,
        }
    }

    pub unsafe fn status(&mut self) -> Result<u8, Error> {
        let mut status = [0];

        self.spi.reset()?;
        self.spi.write(&[0x05])?;
        self.spi.read(&mut status)?;

        Ok(status[0])
    }

    pub unsafe fn status_wait(&mut self, mask: u8, value: u8) -> Result<(), Error> {
        self.timeout.reset();
        while self.timeout.running() {
            if self.status()? & mask == value {
                return Ok(());
            }
        }
        Err(Error::Timeout)
    }

    pub unsafe fn write_disable(&mut self) -> Result<(), Error> {
        self.spi.reset()?;
        self.spi.write(&[0x04])?;

        // Poll status for busy unset and write enable unset
        self.status_wait(3, 0)?;

        Ok(())
    }

    pub unsafe fn write_enable(&mut self) -> Result<(), Error> {
        self.spi.reset()?;
        self.spi.write(&[0x06])?;

        // Poll status for busy unset and write enable set
        self.status_wait(3, 2)?;

        Ok(())
    }

    pub unsafe fn erase_sector(&mut self, address: u32) -> Result<(), Error> {
        if (address & 0xFF00_0000) > 0 {
            return Err(Error::Parameter);
        }

        let instruction = match self.spi.target() {
            SpiTarget::Main => 0xD7,
            SpiTarget::Backup => 0x20,
        };

        self.write_enable()?;

        self.spi.reset()?;
        self.spi.write(&[
            instruction,
            (address >> 16) as u8,
            (address >> 8) as u8,
            address as u8,
        ])?;

        // Poll status for busy unset
        self.status_wait(1, 0)?;

        self.write_disable()?;

        Ok(())
    }

    pub unsafe fn read_at(&mut self, address: u32, data: &mut [u8]) -> Result<usize, Error> {
        if (address & 0xFF00_0000) > 0 {
            return Err(Error::Parameter);
        }

        self.spi.reset()?;
        self.spi.write(&[
            0x0B,
            (address >> 16) as u8,
            (address >> 8) as u8,
            address as u8,
            0,
        ])?;
        self.spi.read(data)
    }

    pub unsafe fn write_at(&mut self, address: u32, data: &[u8]) -> Result<usize, Error> {
        if (address & 0xFF00_0000) > 0 {
            return Err(Error::Parameter);
        }

        self.write_enable()?;

        match self.spi.target() {
            SpiTarget::Main => for (i, word) in data.chunks(2).enumerate() {
                let low = *word.get(0).unwrap_or(&0xFF);
                let high = *word.get(1).unwrap_or(&0xFF);

                self.spi.reset()?;
                if i == 0 {
                    self.spi.write(&[
                        0xAD,
                        (address >> 16) as u8,
                        (address >> 8) as u8,
                        address as u8,
                        low,
                        high
                    ])?;
                } else {
                    self.spi.write(&[
                        0xAD,
                        low,
                        high
                    ])?;
                }

                // Poll status for busy unset
                self.status_wait(1, 0)?;
            },
            SpiTarget::Backup => for (i, page) in data.chunks(256).enumerate() {
                let page_address = address + i as u32 * 256;
                if page_address % 256 != 0 {
                    return Err(Error::Parameter);
                }

                if i > 0 {
                    // Write enable clears after each page is written
                    self.write_enable()?;
                }

                self.spi.reset()?;
                self.spi.write(&[
                    0xF2,
                    (page_address >> 16) as u8,
                    (page_address >> 8) as u8,
                    page_address as u8,
                ])?;
                self.spi.write(&page)?;

                // Poll status for busy unset
                self.status_wait(1, 0)?;
            },
        }

        self.write_disable()?;

        Ok(data.len())
    }
}

impl<'a, S: Spi, T: Timeout> Drop for SpiRom<'a, S, T> {
    fn drop(&mut self) {
        unsafe {
            let _ = self.write_disable();
        }
    }
}
