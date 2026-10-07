//! Readings from the optional sensor service.

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::sensors::{self, Sensor, Unavailable};

pub struct SensorServiceSource {
    /// Off when the readings only add detail to a popup: a missing service
    /// is then not worth reporting.
    pub required: bool,
}

impl Source for SensorServiceSource {
    fn id(&self) -> SourceId {
        SourceId::SensorService
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let readings = match sensors::read() {
            Ok(readings) => readings,
            Err(Unavailable::NotInstalled) if !self.required => return Ok(()),
            Err(e) => return Err(SourceError::Unavailable(e.to_string())),
        };
        for (sensor, key) in [
            (Sensor::CpuTemp, MetricKey::CpuTempC),
            (Sensor::CpuPower, MetricKey::CpuPowerW),
            (Sensor::CpuThrottling, MetricKey::CpuThrottling),
            (Sensor::GpuHotSpot, MetricKey::GpuHotSpotC),
            (Sensor::GpuMemoryTemp, MetricKey::GpuMemTempC),
        ] {
            if let Some(value) = readings.get(sensor) {
                out.set(key, value);
            }
        }
        for (core, temp) in readings.core_temps().into_iter().enumerate() {
            out.set(MetricKey::CpuCoreTempC(core as u16), temp);
        }
        Ok(())
    }
}
