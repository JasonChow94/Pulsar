//! CPU temperature, read from the optional sensor service.

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::sensors;

pub struct CpuSensorsSource;

impl Source for CpuSensorsSource {
    fn id(&self) -> SourceId {
        SourceId::CpuSensors
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let temp = sensors::cpu_temp().map_err(|e| SourceError::Unavailable(e.to_string()))?;
        out.set(MetricKey::CpuTempC, temp);
        Ok(())
    }
}
