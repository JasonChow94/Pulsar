use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use pulsar_core::sensors::{Publisher, Reading};
use windows::Win32::Foundation::ERROR_NOT_SUPPORTED;

use crate::pawnio::PawnIo;

const MODULE: &[u8] = include_bytes!("../../resources/pawnio/IntelMSR.bin");
const MSR_TEMPERATURE_TARGET: u32 = 0x1A2;
const MSR_PACKAGE_THERM_STATUS: u32 = 0x1B1;
const SAMPLE: Duration = Duration::from_secs(1);
/// How often to look for a driver that was missing or went away.
const RETRY: Duration = Duration::from_secs(10);
const STOP_POLL: Duration = Duration::from_millis(100);

/// Package temperature in tenths of a degree: the CPU reports how far it is
/// below its throttling point, which the target register names.
fn package_temp(target: u64, status: u64) -> i32 {
    let tj_max = match (target >> 16) & 0xFF {
        0 => 100,
        t => t as i32,
    };
    let below = ((status >> 16) & 0x7F) as i32;
    (tj_max - below) * 10
}

fn connect() -> Result<PawnIo, Reading> {
    let io = PawnIo::open().map_err(|_| Reading::NoDriver)?;
    io.load(MODULE).map_err(|e| {
        if e.code() == ERROR_NOT_SUPPORTED.to_hresult() {
            Reading::UnsupportedCpu
        } else {
            Reading::ReadFailed
        }
    })?;
    Ok(io)
}

fn read(io: &PawnIo) -> Reading {
    let registers = io
        .read_msr(MSR_TEMPERATURE_TARGET)
        .and_then(|target| Ok((target, io.read_msr(MSR_PACKAGE_THERM_STATUS)?)));
    match registers {
        Ok((target, status)) => Reading::CpuTemp(package_temp(target, status)),
        Err(_) => Reading::ReadFailed,
    }
}

/// Publishes a reading every second until `stop` is set.
pub fn run(stop: &AtomicBool) {
    let Ok(publisher) = Publisher::create() else {
        return;
    };
    let mut session: Result<PawnIo, Reading> = Err(Reading::NoDriver);
    let mut retry_at = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if session.is_err() && Instant::now() >= retry_at {
            session = connect();
            retry_at = Instant::now() + RETRY;
        }
        let reading = match &session {
            Ok(io) => read(io),
            Err(reason) => *reason,
        };
        if session.is_ok() && reading == Reading::ReadFailed {
            session = Err(reading);
        }
        publisher.publish(reading);
        let wake = Instant::now() + SAMPLE;
        while Instant::now() < wake && !stop.load(Ordering::Relaxed) {
            sleep(STOP_POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_is_the_distance_below_tj_max() {
        // TjMax 100 °C, 58 °C below it.
        assert_eq!(package_temp(100 << 16, 0x8000_0000 | 58 << 16), 420);
        assert_eq!(package_temp(90 << 16, 0), 900);
    }

    #[test]
    fn a_missing_tj_max_is_taken_as_100() {
        assert_eq!(package_temp(0, 30 << 16), 700);
    }

    #[test]
    fn the_bundled_module_is_signed() {
        // A signature length, then the signature, then the module.
        let signature = u32::from_le_bytes(MODULE[..4].try_into().unwrap()) as usize;
        assert!(signature > 0 && signature + 4 < MODULE.len());
    }
}
