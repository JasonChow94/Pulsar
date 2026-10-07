//! Running on one physical core at a time, for registers that describe the
//! core they are read on.

use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows::Win32::System::SystemInformation::{
    GROUP_AFFINITY, GetLogicalProcessorInformationEx, RelationProcessorCore,
    SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadGroupAffinity};

/// One logical processor of each physical core, in processor order.
pub fn physical_cores() -> Vec<GROUP_AFFINITY> {
    let mut bytes = 0u32;
    let sized =
        unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut bytes) };
    if sized.is_ok() || sized.is_err_and(|e| e.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult()) {
        return Vec::new();
    }
    // u64 storage keeps the records aligned.
    let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
    let start = buffer
        .as_mut_ptr()
        .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>();
    if unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, Some(start), &mut bytes) }
        .is_err()
    {
        return Vec::new();
    }
    let mut cores = Vec::new();
    let mut offset = 0usize;
    while offset < bytes as usize {
        // Records are variable-length and each states its own size.
        let record = unsafe {
            &*start
                .cast::<u8>()
                .add(offset)
                .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()
        };
        if record.Size == 0 {
            break;
        }
        let group = unsafe { record.Anonymous.Processor.GroupMask[0] };
        cores.push(first_processor(group));
        offset += record.Size as usize;
    }
    cores.sort_by_key(|c| (c.Group, c.Mask));
    cores
}

/// Narrows a core's mask to its lowest logical processor.
fn first_processor(core: GROUP_AFFINITY) -> GROUP_AFFINITY {
    GROUP_AFFINITY {
        Mask: core.Mask & core.Mask.wrapping_neg(),
        Group: core.Group,
        ..Default::default()
    }
}

/// Calls `read` once on each core, then lets the thread run anywhere again.
pub fn on_each<T>(cores: &[GROUP_AFFINITY], mut read: impl FnMut() -> Option<T>) -> Vec<T> {
    let thread = unsafe { GetCurrentThread() };
    let mut original = None;
    let mut values = Vec::with_capacity(cores.len());
    for core in cores {
        let mut previous = GROUP_AFFINITY::default();
        if !unsafe { SetThreadGroupAffinity(thread, core, Some(&mut previous)) }.as_bool() {
            break;
        }
        original.get_or_insert(previous);
        match read() {
            Some(value) => values.push(value),
            None => break,
        }
    }
    if let Some(original) = original {
        unsafe {
            let _ = SetThreadGroupAffinity(thread, &original, None);
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::GetCurrentProcessorNumber;

    #[test]
    fn a_core_is_narrowed_to_its_lowest_processor() {
        let core = GROUP_AFFINITY {
            Mask: 0b1100,
            Group: 1,
            ..Default::default()
        };
        let first = first_processor(core);
        assert_eq!((first.Mask, first.Group), (0b0100, 1));
    }

    #[test]
    fn every_core_is_visited_in_order_on_its_own_processor() {
        let cores = physical_cores();
        assert!(!cores.is_empty());
        assert!(cores.iter().all(|c| c.Mask.count_ones() == 1));
        let ran_on = on_each(&cores, || Some(unsafe { GetCurrentProcessorNumber() }));
        let expected: Vec<u32> = cores
            .iter()
            .filter(|c| c.Group == 0)
            .map(|c| c.Mask.trailing_zeros())
            .collect();
        assert_eq!(ran_on[..expected.len()], expected);
    }

    #[test]
    fn a_failed_read_stops_the_walk() {
        let cores = physical_cores();
        let mut calls = 0;
        let values = on_each(&cores, || {
            calls += 1;
            (calls == 1).then_some(calls)
        });
        assert_eq!(values, [1]);
    }
}
