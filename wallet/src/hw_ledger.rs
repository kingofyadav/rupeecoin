/// Ledger hardware wallet implementation via Linux hidraw kernel interface.
///
/// Opens `/dev/hidraw*` devices directly and identifies Ledger devices by VID/PID
/// using the `HIDIOCGRAWINFO` ioctl.  No `libudev` or `hidapi` crate required.
///
/// # APDU protocol
/// CLA = 0xA5 (RupeeCoin app class byte)
/// INS = 0x01 → GET_PUBLIC_KEY
/// INS = 0x02 → SIGN (path bytes ++ 32-byte tx hash)
///
/// Response status word 0x9000 = OK.  See `HwError` variants for device errors.
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;

use crate::hw_wallet::{DerivationPath, HardwareWallet, HwError, HwProtocol};
use rupeecoin_crypto::Signature;

// ── Ledger VID/PID ────────────────────────────────────────────────────────────

const LEDGER_VID: u16 = 0x2C97;
const LEDGER_PIDS: &[u16] = &[0x0001, 0x0004, 0x0005, 0x0006]; // Nano S/X/S+/Stax

// ── ioctl to read raw HID device info ─────────────────────────────────────────

#[repr(C)]
struct HidrawInfo {
    bus_type: u32,
    vendor: i16,
    product: i16,
}

// HIDIOCGRAWINFO = _IOR('H', 0x03, struct hidraw_devinfo)
// On Linux x86_64: _IOR(type, nr, size) = ((2<<30) | (type<<8) | nr | (size<<16))
const HIDIOCGRAWINFO: u64 =
    (2u64 << 30) | (b'H' as u64) << 8 | 0x03u64 | (std::mem::size_of::<HidrawInfo>() as u64) << 16;

fn get_hidraw_info(fd: i32) -> std::io::Result<HidrawInfo> {
    let mut info = HidrawInfo {
        bus_type: 0,
        vendor: 0,
        product: 0,
    };
    let ret = unsafe {
        // Use the raw `ioctl` syscall without pulling in `libc`.
        ioctl_raw(fd, HIDIOCGRAWINFO, &mut info as *mut HidrawInfo as usize)
    };
    if ret < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(info)
    }
}

/// Issue an ioctl syscall without depending on `libc`.
/// On Linux x86_64, ioctl is syscall 16.
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

// ── HID packet layer ──────────────────────────────────────────────────────────

const HID_PACKET_SIZE: usize = 64;
const HID_CHANNEL: [u8; 2] = [0x01, 0x01];

const CLA: u8 = 0xA5;
pub const INS_GET_PUBLIC_KEY: u8 = 0x01;
pub const INS_SIGN: u8 = 0x02;
const SW_OK: u16 = 0x9000;

// ── LedgerHardwareWallet ─────────────────────────────────────────────────────

pub struct LedgerHardwareWallet {
    dev_path: String,
}

impl LedgerHardwareWallet {
    /// Scan `/dev/hidraw*` for the first Ledger device and return its path.
    pub fn open() -> Result<Self, HwError> {
        for i in 0..16 {
            let path = format!("/dev/hidraw{i}");
            if let Ok(file) = File::open(&path) {
                let fd = file.as_raw_fd();
                if let Ok(info) = get_hidraw_info(fd) {
                    if info.vendor as u16 == LEDGER_VID
                        && LEDGER_PIDS.contains(&(info.product as u16))
                    {
                        return Ok(LedgerHardwareWallet { dev_path: path });
                    }
                }
            }
        }
        Err(HwError::NotConnected(
            "no Ledger device found — connect Ledger via USB and unlock it".into(),
        ))
    }

    /// Send one APDU and receive the response.
    fn exchange(&self, cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Result<Vec<u8>, HwError> {
        let apdu = build_apdu(cla, ins, p1, p2, data);
        let mut dev = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.dev_path)
            .map_err(|e| HwError::Transport(format!("open {}: {e}", self.dev_path)))?;

        send_apdu(&mut dev, &apdu)?;
        recv_apdu(&mut dev)
    }

    fn encode_path(path: &DerivationPath) -> Vec<u8> {
        let mut out = vec![path.indices.len() as u8];
        for &idx in &path.indices {
            out.extend_from_slice(&idx.to_be_bytes());
        }
        out
    }
}

impl HardwareWallet for LedgerHardwareWallet {
    fn protocol(&self) -> HwProtocol {
        HwProtocol::Ledger
    }

    fn get_public_key(&self, path: &DerivationPath) -> Result<Vec<u8>, HwError> {
        let path_bytes = Self::encode_path(path);
        self.exchange(CLA, INS_GET_PUBLIC_KEY, 0, 0, &path_bytes)
    }

    fn sign(&self, data: &[u8], path: &DerivationPath) -> Result<Signature, HwError> {
        let mut payload = Self::encode_path(path);
        payload.extend_from_slice(data);
        let sig_bytes = self.exchange(CLA, INS_SIGN, 0, 0, &payload)?;
        bincode::deserialize(&sig_bytes)
            .map_err(|e| HwError::Transport(format!("signature deserialise: {e}")))
    }

    fn device_info(&self) -> String {
        format!("Ledger ({})", self.dev_path)
    }
}

// ── HID packetisation ─────────────────────────────────────────────────────────

fn build_apdu(cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![cla, ins, p1, p2, data.len() as u8];
    out.extend_from_slice(data);
    out
}

fn send_apdu(dev: &mut File, apdu: &[u8]) -> Result<(), HwError> {
    let mut offset = 0usize;
    let mut seq: u16 = 0;
    let total = apdu.len() as u16;

    loop {
        let mut packet = [0u8; HID_PACKET_SIZE + 1]; // +1 for HID report ID = 0
        let mut pos = 1;
        packet[pos..pos + 2].copy_from_slice(&HID_CHANNEL);
        pos += 2;
        packet[pos] = 0x05; // tag
        pos += 1;
        packet[pos..pos + 2].copy_from_slice(&seq.to_be_bytes());
        pos += 2;
        if seq == 0 {
            packet[pos..pos + 2].copy_from_slice(&total.to_be_bytes());
            pos += 2;
        }
        let space = HID_PACKET_SIZE + 1 - pos;
        let take = (apdu.len() - offset).min(space);
        packet[pos..pos + take].copy_from_slice(&apdu[offset..offset + take]);
        offset += take;
        dev.write_all(&packet)
            .map_err(|e| HwError::Transport(format!("HID write: {e}")))?;
        if offset >= apdu.len() {
            break;
        }
        seq += 1;
    }
    Ok(())
}

fn recv_apdu(dev: &mut File) -> Result<Vec<u8>, HwError> {
    let mut out = Vec::new();
    let mut total: Option<usize> = None;
    let mut seq: u16 = 0;
    let mut buf = [0u8; HID_PACKET_SIZE];

    loop {
        let n = dev
            .read(&mut buf)
            .map_err(|e| HwError::Transport(format!("HID read: {e}")))?;
        if n == 0 {
            return Err(HwError::Transport("Ledger read returned 0 bytes".into()));
        }
        if buf[0..2] != HID_CHANNEL || buf[2] != 0x05 {
            continue;
        }
        let pkt_seq = u16::from_be_bytes([buf[3], buf[4]]);
        if pkt_seq != seq {
            return Err(HwError::Transport("unexpected HID sequence number".into()));
        }
        let mut start = 5;
        if seq == 0 {
            total = Some(u16::from_be_bytes([buf[5], buf[6]]) as usize);
            start = 7;
        }
        let remaining = total.unwrap_or(0).saturating_sub(out.len());
        let take = n.saturating_sub(start).min(remaining);
        out.extend_from_slice(&buf[start..start + take]);
        if out.len() >= total.unwrap_or(0) {
            break;
        }
        seq += 1;
    }

    if out.len() < 2 {
        return Err(HwError::Transport("APDU response too short".into()));
    }
    let sw_off = out.len() - 2;
    let sw = u16::from_be_bytes([out[sw_off], out[sw_off + 1]]);
    let data = out[..sw_off].to_vec();
    match sw {
        SW_OK => Ok(data),
        0x6985 => Err(HwError::NotConnected(
            "Ledger: unlock device and open the RupeeCoin app".into(),
        )),
        0x6E00 => Err(HwError::NotConnected(
            "Ledger: RupeeCoin app not installed — install via Ledger Live".into(),
        )),
        other => Err(HwError::Transport(format!("Ledger APDU SW={other:#06X}"))),
    }
}
