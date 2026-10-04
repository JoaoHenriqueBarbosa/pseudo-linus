//! Dispositivos de caractere (`drivers/char/mem.c`, `random.c`, `tty_io.c`), escolhidos pelo `rdev` do
//! inode, como o `chrdev_open`.

use sysabi::Errno;
use vfs::makedev;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Device {
    Null,
    Zero,
    Full,
    Random,
    Urandom,
}

pub(crate) const DEV_NULL: (u32, u32) = (1, 3);
pub(crate) const DEV_ZERO: (u32, u32) = (1, 5);
pub(crate) const DEV_FULL: (u32, u32) = (1, 7);
pub(crate) const DEV_RANDOM: (u32, u32) = (1, 8);
pub(crate) const DEV_URANDOM: (u32, u32) = (1, 9);
pub(crate) const DEV_TTY: (u32, u32) = (5, 0);

impl Device {
    /// Driver do `rdev`. `/dev/tty` sem terminal de controle dá ENXIO; número sem driver também.
    pub(crate) fn open(rdev: u64) -> Result<Device, Errno> {
        let pick = |d: (u32, u32)| makedev(d.0, d.1) == rdev;
        if pick(DEV_NULL) {
            Ok(Device::Null)
        } else if pick(DEV_ZERO) {
            Ok(Device::Zero)
        } else if pick(DEV_FULL) {
            Ok(Device::Full)
        } else if pick(DEV_RANDOM) {
            Ok(Device::Random)
        } else if pick(DEV_URANDOM) {
            Ok(Device::Urandom)
        } else {
            Err(Errno::ENXIO)
        }
    }

    pub(crate) fn read(self, buf: &mut [u8]) -> Result<usize, Errno> {
        match self {
            Device::Null => Ok(0),
            Device::Zero | Device::Full => {
                buf.fill(0);
                Ok(buf.len())
            }
            Device::Random | Device::Urandom => {
                getrandom::fill(buf).map_err(|_| Errno::EIO)?;
                Ok(buf.len())
            }
        }
    }

    pub(crate) fn write(self, buf: &[u8]) -> Result<usize, Errno> {
        match self {
            Device::Full => Err(Errno::ENOSPC),
            _ => Ok(buf.len()),
        }
    }

    /// `lseek`: null e zero aceitam e voltam 0 (`null_lseek`/`noop_llseek` deixam em 0); random também.
    pub(crate) fn lseek(self) -> Result<u64, Errno> {
        Ok(0)
    }
}
