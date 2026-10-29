// SPDX-License-Identifier: MIT
use crate::{
    Backend, Error, ErrorKind, Plan,
    model::{Model, VID},
    state::{Change, MAX_CHANGES, STATE_SIZE, State, predict},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::fd::AsRawFd,
};

pub const NODE: &str = "/dev/minifuse_cmd";
pub const TX_SIZE: usize = 8 + STATE_SIZE + MAX_CHANGES * 4 + STATE_SIZE + 4;
const AFTER: usize = 8 + STATE_SIZE + MAX_CHANGES * 4;
const FLAGS: usize = TX_SIZE - 2;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub vendor_id: u16,
    pub product_id: u16,
    pub bus: u16,
    pub address: u16,
}

impl Device {
    pub fn require_mf4(&self) -> Result<(), String> {
        if self.vendor_id != VID {
            return Err("Unexpected USB vendor in device record".into());
        }
        Model::from_pid(self.product_id)
            .ok_or("Unsupported MiniFuse product ID")?
            .require_mixer()
    }
}

pub struct Kernel {
    file: File,
}

fn word(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}
fn put(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub fn decode_record(data: &[u8]) -> Result<(Device, State), Error> {
    if data.len() != 206
        || &data[..4] != b"MF4S"
        || word(data, 4) != 1
        || word(data, 14) != STATE_SIZE as u16
    {
        return Err(Error::new(
            ErrorKind::ModuleIncompatible,
            "Unsupported kernel status record; rebuild/reload the matching mf-cli module",
        ));
    }
    let device = Device {
        vendor_id: word(data, 6),
        product_id: word(data, 8),
        bus: word(data, 10),
        address: word(data, 12),
    };
    device
        .require_mf4()
        .map_err(|e| Error::new(ErrorKind::UnsupportedCapability, e))?;
    let mut state = [0; STATE_SIZE];
    state.copy_from_slice(&data[16..]);
    Ok((device, State(state)))
}

impl Kernel {
    pub fn open(write: bool) -> Result<Self, Error> {
        OpenOptions::new().read(true).write(write).open(NODE).map(|file| Self { file })
            .map_err(|e| Error::io(e, &format!("Cannot open {NODE}; status/mixer/config require the matching kernel module and device access; no USB fallback is used")))
    }
    pub fn read(&mut self) -> Result<(Device, State), Error> {
        let mut data = [0; 206];
        self.file.read_exact(&mut data).map_err(|e| {
            Error::io(
                e,
                "Cannot read MF4 status; only PID af70 and the matching module are supported",
            )
        })?;
        decode_record(&data)
    }
}

pub fn encode_transaction(
    device: &Device,
    before: &State,
    changes: &[Change],
) -> Result<[u8; TX_SIZE], String> {
    device.require_mf4()?;
    if changes.is_empty() {
        return Err("An empty transaction must not be sent".into());
    }
    predict(before, changes)?;
    let mut data = [0; TX_SIZE];
    put(&mut data, 0, 1);
    put(&mut data, 2, device.bus);
    put(&mut data, 4, device.address);
    put(&mut data, 6, changes.len() as u16);
    data[8..8 + STATE_SIZE].copy_from_slice(&before.0);
    for (i, c) in changes.iter().enumerate() {
        put(&mut data, 8 + STATE_SIZE + i * 4, c.selector);
        put(&mut data, 10 + STATE_SIZE + i * 4, c.value);
    }
    Ok(data)
}

impl Backend for Kernel {
    fn transact(&mut self, plan: &Plan) -> Result<State, Error> {
        let device = &plan.device;
        let before = &plan.before;
        let changes = &plan.changes;
        let expected = predict(before, changes)?;
        let mut data = encode_transaction(device, before, changes)?;
        let request = libc::_IOWR::<[u8; TX_SIZE]>(b'M' as u32, 0x41);
        // The kernel copies a fixed-size, little-endian byte buffer. No pointers
        // or Rust layout-dependent structures cross this ABI.
        let result = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                request as libc::c_ulong,
                data.as_mut_ptr(),
            )
        };
        if result < 0 {
            let error = std::io::Error::last_os_error();
            let flags = word(&data, FLAGS);
            let recovery = if flags & 2 != 0 {
                "The previous complete B2 state was restored and verified."
            } else if flags & 1 != 0 {
                "Restoration was attempted but could not be verified."
            } else {
                "The transaction was rejected or failed before rollback was available; refresh status."
            };
            let mut failure = Error::io(error, "MF4 transaction failed");
            failure.message.push_str(&format!(". {recovery}"));
            failure.rollback_attempted = flags & 1 != 0;
            failure.rollback_verified = flags & 2 != 0;
            failure.device_write_attempted = if failure.rollback_attempted {
                Some(true)
            } else if matches!(
                failure.kind,
                ErrorKind::AccessDenied
                    | ErrorKind::AmbiguousDevice
                    | ErrorKind::ModuleIncompatible
                    | ErrorKind::UnsupportedCapability
                    | ErrorKind::DeviceChanged
                    | ErrorKind::StaleState
            ) {
                Some(false)
            } else {
                // A failed copy/USB/read call does not prove non-dispatch.
                None
            };
            if flags & 4 != 0 {
                let mut after = [0; STATE_SIZE];
                after.copy_from_slice(&data[AFTER..AFTER + STATE_SIZE]);
                failure.after_state = Some(Box::new(State(after)));
            }
            return Err(failure);
        }
        if word(&data, FLAGS) & 4 == 0 || data[AFTER..AFTER + STATE_SIZE] != expected.0 {
            let mut error = Error::new(
                ErrorKind::ReadbackFailed,
                "Kernel response did not confirm the predicted complete B2 state",
            );
            error.device_write_attempted = Some(true);
            return Err(error);
        }
        Ok(expected)
    }
}

#[cfg(test)]
pub fn reference_device() -> Device {
    Device {
        vendor_id: VID,
        product_id: crate::model::MF4_PID,
        bus: 0,
        address: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MF4_PID;
    #[test]
    fn rejects_malformed_records_and_other_models() {
        assert!(decode_record(&[]).is_err());
        let mut record = [0; 206];
        record[..4].copy_from_slice(b"MF4S");
        for (offset, value) in [(4, 1), (6, VID), (8, MF4_PID), (14, 190)] {
            put(&mut record, offset, value);
        }
        assert!(decode_record(&record).is_ok());
        for pid in [0xaf80, 0xaf82, 0xaf90, 0xaf72] {
            put(&mut record, 8, pid);
            assert!(decode_record(&record).is_err());
        }
    }
    #[test]
    fn transaction_layout_matches_fixed_abi() {
        let state = State([0; STATE_SIZE]);
        let d = Device {
            bus: 2,
            address: 5,
            ..reference_device()
        };
        let b = encode_transaction(
            &d,
            &state,
            &[Change {
                selector: 0x0900,
                value: 0x8000,
            }],
        )
        .unwrap();
        assert_eq!(TX_SIZE, 1032);
        assert_eq!(AFTER, 838);
        assert_eq!(&b[..8], &[1, 0, 2, 0, 5, 0, 1, 0]);
        assert_eq!(&b[198..202], &[0, 9, 0, 128]);
    }
}
