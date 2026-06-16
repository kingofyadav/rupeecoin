/// Trezor hardware wallet — Phase 15 HID implementation.
///
/// Enumerates `/dev/hidraw*` for Trezor devices (VID 0x534C or 0x1209),
/// performs an Initialize handshake over Trezor Wire Protocol v1 to verify
/// connectivity, and surfaces a clear error for signing because a custom
/// RupeeCoin Dilithium3 Trezor firmware app does not yet exist.
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;

use crate::hw_wallet::{DerivationPath, HardwareWallet, HwError, HwProtocol};
use rupeecoin_crypto::Signature;

// ── Trezor VID/PID ────────────────────────────────────────────────────────────

const TREZOR_VID_SATOSHILABS: u16 = 0x534C; // Model One
const TREZOR_VID_GENERIC: u16 = 0x1209;     // T / Safe 3 / Safe 5 (pid.codes shared VID)
const TREZOR_GENERIC_PIDS: &[u16] = &[0x53C1, 0x53C3, 0x53C5]; // T, Safe 3, Safe 5

// ── ioctl to read raw HID device info ─────────────────────────────────────────

#[repr(C)]
struct HidrawInfo {
    bus_type: u32,
    vendor: i16,
    product: i16,
}

// HIDIOCGRAWINFO on Linux x86_64
const HIDIOCGRAWINFO: u64 = (2u64 << 30)
    | (b'H' as u64) << 8
    | 0x03u64
    | (std::mem::size_of::<HidrawInfo>() as u64) << 16;

fn get_hidraw_info(fd: i32) -> std::io::Result<HidrawInfo> {
    let mut info = HidrawInfo { bus_type: 0, vendor: 0, product: 0 };
    let ret = unsafe { ioctl_raw(fd, HIDIOCGRAWINFO, &mut info as *mut HidrawInfo as usize) };
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(info)
    }
}

unsafe fn ioctl_raw(fd: i32, request: u64, arg: usize) -> i32 {
    let ret: i64;
    std::arch::asm!(
        "syscall",
        inlateout("rax") 16i64 => ret,
        in("rdi") fd as i64,
        in("rsi") request,
        in("rdx") arg,
        out("rcx") _,
        out("r11") _,
        options(nostack),
    );
    ret as i32
}

fn is_trezor(info: &HidrawInfo) -> bool {
    let vid = info.vendor as u16;
    let pid = info.product as u16;
    vid == TREZOR_VID_SATOSHILABS
        || (vid == TREZOR_VID_GENERIC && TREZOR_GENERIC_PIDS.contains(&pid))
}

// ── Trezor Wire Protocol v1 framing ──────────────────────────────────────────
//
// Each HID report is 64 bytes.
// First write packet:  [0x3F, 0x23, 0x23, type_hi, type_lo, len3, len2, len1, len0, data...]
// Continuation:        [0x3F, data...]
//
// MessageType_Initialize = 0
// MessageType_Features   = 17

const TREZOR_PACKET_SIZE: usize = 64;
const TREZOR_MSG_FEATURES: u16 = 17;

fn trezor_write(dev: &mut File, msg_type: u16, data: &[u8]) -> Result<(), HwError> {
    let total_len = data.len() as u32;
    let mut offset = 0usize;
    let mut first = true;

    loop {
        let mut pkt = [0u8; TREZOR_PACKET_SIZE];
        let pos;

        if first {
            pkt[0] = 0x3F;
            pkt[1] = 0x23;
            pkt[2] = 0x23;
            pkt[3..5].copy_from_slice(&msg_type.to_be_bytes());
            pkt[5..9].copy_from_slice(&total_len.to_be_bytes());
            pos = 9;
            first = false;
        } else {
            pkt[0] = 0x3F;
            pos = 1;
        }

        let take = (data.len() - offset).min(TREZOR_PACKET_SIZE - pos);
        if take > 0 {
            pkt[pos..pos + take].copy_from_slice(&data[offset..offset + take]);
            offset += take;
        }

        dev.write_all(&pkt)
            .map_err(|e| HwError::Transport(format!("Trezor HID write: {e}")))?;

        if offset >= data.len() {
            break;
        }
    }
    Ok(())
}

fn trezor_read(dev: &mut File) -> Result<(u16, Vec<u8>), HwError> {
    let mut buf = [0u8; TREZOR_PACKET_SIZE];

    dev.read_exact(&mut buf)
        .map_err(|e| HwError::Transport(format!("Trezor HID read: {e}")))?;

    if buf[0] != 0x3F || buf[1] != 0x23 || buf[2] != 0x23 {
        return Err(HwError::Transport("Trezor: invalid response framing".into()));
    }

    let msg_type = u16::from_be_bytes([buf[3], buf[4]]);
    let total_len = u32::from_be_bytes([buf[5], buf[6], buf[7], buf[8]]) as usize;

    let avail = TREZOR_PACKET_SIZE - 9;
    let first_take = avail.min(total_len);
    let mut data = buf[9..9 + first_take].to_vec();

    while data.len() < total_len {
        dev.read_exact(&mut buf)
            .map_err(|e| HwError::Transport(format!("Trezor HID read: {e}")))?;
        if buf[0] != 0x3F {
            return Err(HwError::Transport("Trezor: invalid continuation packet".into()));
        }
        let remaining = total_len - data.len();
        let take = (TREZOR_PACKET_SIZE - 1).min(remaining);
        data.extend_from_slice(&buf[1..1 + take]);
    }

    Ok((msg_type, data))
}

// ── TrezorHardwareWallet ──────────────────────────────────────────────────────

pub struct TrezorHardwareWallet {
    dev_path: String,
}

impl TrezorHardwareWallet {
    /// Scan `/dev/hidraw*` for a Trezor device, send an Initialize handshake
    /// to verify it responds, and return the handle.
    pub fn open() -> Result<Self, HwError> {
        for i in 0..16 {
            let path = format!("/dev/hidraw{i}");
            if let Ok(file) = File::open(&path) {
                let fd = file.as_raw_fd();
                if let Ok(info) = get_hidraw_info(fd) {
                    if !is_trezor(&info) {
                        continue;
                    }
                    // Send Initialize (msg_type=0, no payload) to confirm the device responds.
                    let mut dev = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(&path)
                        .map_err(|e| HwError::Transport(format!("open {path}: {e}")))?;
                    trezor_write(&mut dev, 0, &[])?;
                    match trezor_read(&mut dev) {
                        Ok((TREZOR_MSG_FEATURES, _)) => {
                            return Ok(TrezorHardwareWallet { dev_path: path });
                        }
                        Ok((msg_type, _)) => {
                            return Err(HwError::Transport(format!(
                                "Trezor: unexpected response msg_type={msg_type} to Initialize"
                            )));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
        }
        Err(HwError::NotConnected(
            "no Trezor device found — connect Trezor via USB and unlock it".into(),
        ))
    }
}

impl HardwareWallet for TrezorHardwareWallet {
    fn protocol(&self) -> HwProtocol {
        HwProtocol::Trezor
    }

    fn get_public_key(&self, _path: &DerivationPath) -> Result<Vec<u8>, HwError> {
        Err(HwError::NotConnected(
            "Trezor: RupeeCoin Dilithium3 firmware app not yet available — \
             signing requires a custom Trezor partner firmware (Phase 15 roadmap item)"
                .into(),
        ))
    }

    fn sign(&self, _data: &[u8], _path: &DerivationPath) -> Result<Signature, HwError> {
        Err(HwError::NotConnected(
            "Trezor: RupeeCoin Dilithium3 firmware app not yet available — \
             signing requires a custom Trezor partner firmware (Phase 15 roadmap item)"
                .into(),
        ))
    }

    fn device_info(&self) -> String {
        format!("Trezor ({})", self.dev_path)
    }
}
