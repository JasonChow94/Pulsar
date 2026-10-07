//! CPU temperature and power, read from the optional sensor service.

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::sensors::{self, Sensor};

pub struct CpuSensorsSource;

impl Source for CpuSensorsSource {
    fn id(&self) -> SourceId {
        SourceId::CpuSensors
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let readings = sensors::read().map_err(|e| SourceError::Unavailable(e.to_string()))?;
        for (sensor, key) in [
            (Sensor::CpuTemp, MetricKey::CpuTempC),
            (Sensor::CpuPower, MetricKey::CpuPowerW),
        ] {
            if let Some(value) = readings.get(sensor) {
                out.set(key, value);
            }
        }
        Ok(())
    }
}
