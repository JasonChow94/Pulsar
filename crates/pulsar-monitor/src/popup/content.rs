//! What a hover popup shows for an item, independent of how it is drawn.

use pulsar_core::format::format_value;
use pulsar_core::history::HistoryStore;
use pulsar_core::layout::CellPart;
use pulsar_core::metric::{GpuEngineKind, ItemKind, MetricKey, Snapshot, SourceId, Unit};
use pulsar_core::sources::{ProcessSort, top_by};

use pulsar_core::display::cell_value;

const TOP_PROCESSES: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Rows {
        heading: Option<&'static str>,
        rows: Vec<(String, String)>,
    },
    /// One bar per value, each 0–100 (per-core CPU).
    Bars {
        heading: &'static str,
        values: Vec<f64>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopupContent {
    pub kind: ItemKind,
    pub title: &'static str,
    pub value: String,
    /// Graph series, drawn in order; the first is filled.
    pub series: Vec<CellPart>,
    pub error: Option<String>,
    pub blocks: Vec<Block>,
}

/// Whether the popup lists processes, so the sampler must collect them.
pub fn needs_processes(kind: ItemKind) -> bool {
    matches!(kind, ItemKind::Cpu | ItemKind::Ram | ItemKind::Gpu)
}

fn fmt(snapshot: &Snapshot, key: MetricKey) -> String {
    format_value(snapshot.get(key).unwrap_or(f64::NAN), key.unit())
}

fn of(snapshot: &Snapshot, used: MetricKey, total: MetricKey) -> String {
    format!("{} / {}", fmt(snapshot, used), fmt(snapshot, total))
}

fn processes(snapshot: &Snapshot, sort: ProcessSort) -> Block {
    let top = top_by(&snapshot.processes, sort, TOP_PROCESSES);
    let note = |text: &str| vec![(text.to_string(), String::new())];
    let rows = if let Some(reason) = snapshot.errors.get(&SourceId::Processes) {
        note(reason)
    } else if snapshot.processes.is_empty() {
        note("正在收集…")
    } else if top.is_empty() {
        note("无活动")
    } else {
        top.into_iter()
            .map(|p| {
                let value = match sort {
                    ProcessSort::Cpu => format_value(p.cpu_percent, Unit::Percent),
                    ProcessSort::Memory => format_value(p.private_bytes, Unit::Bytes),
                    ProcessSort::Gpu => format_value(p.gpu_percent, Unit::Percent),
                };
                (p.name, value)
            })
            .collect()
    };
    Block::Rows {
        heading: Some("占用最高的进程"),
        rows,
    }
}

fn cores(snapshot: &Snapshot) -> Vec<f64> {
    (0u16..)
        .map_while(|i| snapshot.get(MetricKey::CpuCore(i)))
        .collect()
}

fn volumes(snapshot: &Snapshot) -> Vec<(String, String)> {
    (b'A'..=b'Z')
        .filter_map(|letter| {
            let free = snapshot.get(MetricKey::VolumeFreeBytes(letter))?;
            let total = snapshot.get(MetricKey::VolumeTotalBytes(letter))?;
            Some((
                format!("{}:", letter as char),
                format!(
                    "可用 {}，共 {}",
                    format_value(free, Unit::Bytes),
                    format_value(total, Unit::Bytes)
                ),
            ))
        })
        .collect()
}

fn engines(snapshot: &Snapshot) -> Vec<(String, String)> {
    [
        (GpuEngineKind::ThreeD, "3D"),
        (GpuEngineKind::Compute, "计算"),
        (GpuEngineKind::VideoDecode, "视频解码"),
        (GpuEngineKind::VideoEncode, "视频编码"),
        (GpuEngineKind::Copy, "复制"),
    ]
    .into_iter()
    .filter_map(|(kind, label)| {
        let v = snapshot.get(MetricKey::GpuEngine(kind))?;
        Some((label.to_string(), format_value(v, Unit::Percent)))
    })
    .collect()
}

fn ping_stats(history: &HistoryStore) -> String {
    let h = history.get(MetricKey::PingMs);
    let stat = |f: fn(&pulsar_core::history::History) -> Option<f64>| {
        h.and_then(f).map_or("—".to_string(), |v| format!("{v:.0}"))
    };
    format!(
        "{} / {} / {} ms",
        stat(|h| h.min()),
        stat(|h| h.avg()),
        stat(|h| h.max())
    )
}

fn sensors(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut rows = vec![
        ("风扇".into(), fmt(snapshot, MetricKey::GpuFanRpm)),
        ("功耗".into(), fmt(snapshot, MetricKey::GpuPowerPercent)),
        ("显存频率".into(), fmt(snapshot, MetricKey::GpuMemClockMhz)),
    ];
    // Only the sensor service reports these, and only for some cards.
    for (label, key) in [
        ("热点", MetricKey::GpuHotSpotC),
        ("显存温度", MetricKey::GpuMemTempC),
    ] {
        if let Some(value) = snapshot.get(key) {
            rows.push((label.into(), format_value(value, key.unit())));
        }
    }
    rows
}

fn core_temps(snapshot: &Snapshot) -> Vec<f64> {
    (0..)
        .map_while(|i| snapshot.get(MetricKey::CpuCoreTempC(i)))
        .collect()
}

/// Whatever the sensor service reported; nothing without it.
fn cpu_sensors(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let mut row = |label: &str, key: MetricKey| {
        if let Some(value) = snapshot.get(key) {
            rows.push((label.to_string(), format_value(value, key.unit())));
        }
    };
    row("温度", MetricKey::CpuTempC);
    row("功耗", MetricKey::CpuPowerW);
    if let Some(hottest) = core_temps(snapshot).into_iter().reduce(f64::max) {
        rows.push(("最热核心".into(), format_value(hottest, Unit::Celsius)));
    }
    if let Some(throttling) = snapshot.get(MetricKey::CpuThrottling) {
        let text = if throttling > 0.0 { "是" } else { "否" };
        rows.push(("过热降频".into(), text.into()));
    }
    rows
}

fn drive_rows(snapshot: &Snapshot, drives: &[u8]) -> Vec<(String, String)> {
    drives
        .iter()
        .map(|&d| {
            let active = snapshot.get(MetricKey::DriveActivePercent(d));
            let used = snapshot.get(MetricKey::DriveUsedPercent(d));
            let value = match (active, used) {
                (None, None) => "—".to_string(),
                (a, u) => format!(
                    "活动 {} · 已用 {}",
                    format_value(a.unwrap_or(f64::NAN), Unit::Percent),
                    format_value(u.unwrap_or(f64::NAN), Unit::Percent)
                ),
            };
            (format!("{}:", d as char), value)
        })
        .collect()
}

/// `drives` are the letters shown as separate disk cells.
pub fn build(
    kind: ItemKind,
    snapshot: &Snapshot,
    history: &HistoryStore,
    ping_host: &str,
    drives: &[u8],
) -> PopupContent {
    let error = snapshot.errors.get(&kind.source()).cloned();
    let (title, value, series, mut blocks) = match kind {
        ItemKind::Cpu => (
            "CPU",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: [("频率".into(), fmt(snapshot, MetricKey::CpuClockMhz))]
                        .into_iter()
                        .chain(cpu_sensors(snapshot))
                        .collect(),
                },
                Block::Bars {
                    heading: "核心",
                    values: cores(snapshot),
                },
                processes(snapshot, ProcessSort::Cpu),
            ],
        ),
        ItemKind::Ram => (
            "内存",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: vec![
                        (
                            "已用".into(),
                            of(snapshot, MetricKey::MemUsedBytes, MetricKey::MemTotalBytes),
                        ),
                        ("可用".into(), fmt(snapshot, MetricKey::MemAvailableBytes)),
                        (
                            "已提交".into(),
                            of(
                                snapshot,
                                MetricKey::CommitBytes,
                                MetricKey::CommitLimitBytes,
                            ),
                        ),
                    ],
                },
                processes(snapshot, ProcessSort::Memory),
            ],
        ),
        ItemKind::Disk => (
            "磁盘",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: vec![
                        ("读取".into(), fmt(snapshot, MetricKey::DiskReadBps)),
                        ("写入".into(), fmt(snapshot, MetricKey::DiskWriteBps)),
                    ],
                },
                Block::Rows {
                    heading: Some("驱动器"),
                    rows: drive_rows(snapshot, drives),
                },
                Block::Rows {
                    heading: Some("可用空间"),
                    rows: match snapshot.errors.get(&SourceId::DiskSpace) {
                        Some(reason) => vec![(reason.clone(), String::new())],
                        None => volumes(snapshot),
                    },
                },
            ],
        ),
        ItemKind::Network => {
            let adapters: Vec<(String, String)> = snapshot
                .adapters
                .iter()
                .map(|a| {
                    (
                        a.name.clone(),
                        format!(
                            "↓ {}  ↑ {}",
                            format_value(a.down_bps, Unit::BytesPerSec),
                            format_value(a.up_bps, Unit::BytesPerSec)
                        ),
                    )
                })
                .collect();
            (
                "网络",
                format!(
                    "↓ {}  ↑ {}",
                    cell_value(kind, CellPart::Down, snapshot),
                    cell_value(kind, CellPart::Up, snapshot)
                ),
                vec![CellPart::Down, CellPart::Up],
                vec![Block::Rows {
                    heading: Some("网卡"),
                    rows: if adapters.is_empty() {
                        vec![("无流量".into(), String::new())]
                    } else {
                        adapters
                    },
                }],
            )
        }
        ItemKind::Gpu => (
            "GPU",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: Some("引擎"),
                    rows: engines(snapshot),
                },
                Block::Rows {
                    heading: Some("传感器"),
                    rows: match snapshot.get(MetricKey::GpuTempC) {
                        Some(_) => std::iter::once((
                            "温度".to_string(),
                            fmt(snapshot, MetricKey::GpuTempC),
                        ))
                        .chain(sensors(snapshot))
                        .collect(),
                        None => Vec::new(),
                    },
                },
                Block::Rows {
                    heading: Some("显存"),
                    rows: vec![
                        ("专用".into(), fmt(snapshot, MetricKey::GpuDedicatedBytes)),
                        ("共享".into(), fmt(snapshot, MetricKey::GpuSharedBytes)),
                    ],
                },
                processes(snapshot, ProcessSort::Gpu),
            ],
        ),
        ItemKind::GpuTemp => (
            "GPU 温度",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![Block::Rows {
                heading: None,
                rows: sensors(snapshot),
            }],
        ),
        ItemKind::CpuTemp | ItemKind::CpuPower => (
            if kind == ItemKind::CpuTemp {
                "CPU 温度"
            } else {
                "CPU 功耗"
            },
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: cpu_sensors(snapshot),
                },
                Block::Bars {
                    heading: "核心温度",
                    values: core_temps(snapshot),
                },
            ],
        ),
        ItemKind::Ping => (
            "延迟",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![Block::Rows {
                heading: None,
                rows: vec![
                    ("主机".into(), ping_host.to_string()),
                    ("最小 / 平均 / 最大".into(), ping_stats(history)),
                    ("丢包率".into(), fmt(snapshot, MetricKey::PingLossPercent)),
                ],
            }],
        ),
    };
    // Optional sections vanish rather than showing a bare heading.
    blocks.retain(|b| {
        !matches!(b, Block::Rows { heading: Some("驱动器" | "传感器"), rows } if rows.is_empty())
    });
    PopupContent {
        kind,
        title,
        value,
        series,
        error,
        blocks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_core::metric::{AdapterRate, ProcessUsage};

    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    fn rows(block: &Block) -> &[(String, String)] {
        match block {
            Block::Rows { rows, .. } => rows,
            Block::Bars { .. } => panic!("expected rows"),
        }
    }

    fn proc(name: &str, cpu: f64, mem: f64, gpu: f64) -> ProcessUsage {
        ProcessUsage {
            name: name.into(),
            cpu_percent: cpu,
            private_bytes: mem,
            gpu_percent: gpu,
        }
    }

    #[test]
    fn cpu_shows_speed_cores_and_top_processes() {
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, 23.0);
        s.set(MetricKey::CpuClockMhz, 3920.0);
        for (i, v) in [10.0, 90.0, 40.0].into_iter().enumerate() {
            s.set(MetricKey::CpuCore(i as u16), v);
        }
        s.processes = vec![
            proc("idle", 0.0, 1.0, 0.0),
            proc("code", 12.0, 1.0, 0.0),
            proc("chrome", 30.0, 1.0, 0.0),
        ];
        let c = build(ItemKind::Cpu, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!((c.title, c.value.as_str()), ("CPU", "23%"));
        assert_eq!(rows(&c.blocks[0])[0], ("频率".into(), "3.92 GHz".into()));
        assert_eq!(
            c.blocks[1],
            Block::Bars {
                heading: "核心",
                values: vec![10.0, 90.0, 40.0]
            }
        );
        let top = rows(&c.blocks[2]);
        assert_eq!(top[0], ("chrome".into(), "30%".into()));
        assert_eq!(top.len(), 2, "processes at zero are left out");
    }

    #[test]
    fn process_list_says_collecting_before_first_sample() {
        let c = build(
            ItemKind::Ram,
            &Snapshot::default(),
            &HistoryStore::new(60),
            "",
            &[],
        );
        assert_eq!(
            rows(&c.blocks[1]),
            [("正在收集…".to_string(), String::new())]
        );
    }

    #[test]
    fn idle_process_list_says_no_activity_not_collecting() {
        let s = Snapshot {
            processes: vec![proc("idle", 0.0, 1.0, 0.0)],
            ..Snapshot::default()
        };
        let c = build(ItemKind::Gpu, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!(rows(&c.blocks[2]), [("无活动".to_string(), String::new())]);
    }

    #[test]
    fn failed_process_source_shows_its_reason() {
        let mut s = Snapshot::default();
        s.errors
            .insert(SourceId::Processes, "Process counters unavailable".into());
        let c = build(ItemKind::Cpu, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!(rows(&c.blocks[2])[0].0, "Process counters unavailable");
    }

    #[test]
    fn failed_disk_space_shows_its_reason() {
        let mut s = Snapshot::default();
        s.errors.insert(SourceId::DiskSpace, "Access denied".into());
        let c = build(ItemKind::Disk, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!(
            rows(&c.blocks[1]),
            [("Access denied".to_string(), String::new())]
        );
    }

    #[test]
    fn memory_shows_used_of_total_and_commit() {
        let mut s = Snapshot::default();
        s.set(MetricKey::MemUsedBytes, 26.2 * GB);
        s.set(MetricKey::MemTotalBytes, 31.8 * GB);
        s.set(MetricKey::CommitBytes, 50.0 * GB);
        s.set(MetricKey::CommitLimitBytes, 62.0 * GB);
        let c = build(ItemKind::Ram, &s, &HistoryStore::new(60), "", &[]);
        let r = rows(&c.blocks[0]);
        assert_eq!(r[0].1, "26.2 GB / 31.8 GB");
        assert_eq!(r[2].1, "50.0 GB / 62.0 GB");
        assert_eq!(r[1].1, "—", "missing metric shows a dash");
    }

    #[test]
    fn disk_lists_volumes_in_letter_order() {
        let mut s = Snapshot::default();
        for (letter, free) in [(b'D', 100.0), (b'C', 269.0)] {
            s.set(MetricKey::VolumeFreeBytes(letter), free * GB);
            s.set(MetricKey::VolumeTotalBytes(letter), 931.0 * GB);
        }
        let c = build(ItemKind::Disk, &s, &HistoryStore::new(60), "", &[]);
        let v = rows(&c.blocks[1]);
        assert_eq!(v[0], ("C:".into(), "可用 269 GB，共 931 GB".into()));
        assert_eq!(v[1].0, "D:");
    }

    #[test]
    fn network_shows_both_directions_and_adapters() {
        let mut s = Snapshot::default();
        s.set(MetricKey::NetDownBps, 2048.0);
        s.set(MetricKey::NetUpBps, 512.0);
        s.adapters = vec![AdapterRate {
            name: "Realtek".into(),
            down_bps: 2048.0,
            up_bps: 512.0,
        }];
        let c = build(ItemKind::Network, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!(c.value, "↓ 2.0 KB/s  ↑ 512 B/s");
        assert_eq!(c.series, vec![CellPart::Down, CellPart::Up]);
        assert_eq!(
            rows(&c.blocks[0])[0],
            ("Realtek".into(), "↓ 2.0 KB/s  ↑ 512 B/s".into())
        );

        let idle = build(
            ItemKind::Network,
            &Snapshot::default(),
            &HistoryStore::new(60),
            "",
            &[],
        );
        assert_eq!(rows(&idle.blocks[0])[0].0, "无流量");
    }

    #[test]
    fn gpu_lists_only_reported_engines() {
        let mut s = Snapshot::default();
        s.set(MetricKey::GpuEngine(GpuEngineKind::ThreeD), 7.0);
        s.set(MetricKey::GpuEngine(GpuEngineKind::VideoDecode), 2.0);
        let c = build(ItemKind::Gpu, &s, &HistoryStore::new(60), "", &[]);
        let labels: Vec<&str> = rows(&c.blocks[0]).iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["3D", "视频解码"]);
    }

    #[test]
    fn ping_shows_host_and_history_stats() {
        let mut history = HistoryStore::new(60);
        for ms in [12.0, 20.0, 16.0] {
            let mut s = Snapshot::default();
            s.set(MetricKey::PingMs, ms);
            history.record(&s);
        }
        let c = build(
            ItemKind::Ping,
            &Snapshot::default(),
            &history,
            "1.1.1.1",
            &[],
        );
        let r = rows(&c.blocks[0]);
        assert_eq!(r[0].1, "1.1.1.1");
        assert_eq!(r[1].1, "12 / 16 / 20 ms");
    }

    #[test]
    fn failed_source_reason_is_shown() {
        let mut s = Snapshot::default();
        s.errors
            .insert(SourceId::Gpu, "GPU counters unavailable".into());
        let c = build(ItemKind::Gpu, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!(c.error.as_deref(), Some("GPU counters unavailable"));
        assert_eq!(c.value, "—");
    }

    #[test]
    fn process_lists_only_for_cpu_ram_gpu() {
        let with: Vec<ItemKind> = ItemKind::ALL
            .into_iter()
            .filter(|&k| needs_processes(k))
            .collect();
        assert_eq!(with, [ItemKind::Cpu, ItemKind::Ram, ItemKind::Gpu]);
    }

    #[test]
    fn gpu_temperature_popup_lists_the_sensors() {
        let mut s = Snapshot::default();
        s.set(MetricKey::GpuTempC, 49.1);
        s.set(MetricKey::GpuFanRpm, 0.0);
        s.set(MetricKey::GpuPowerPercent, 10.6);
        s.set(MetricKey::GpuMemClockMhz, 7001.0);
        let c = build(ItemKind::GpuTemp, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!(c.title, "GPU 温度");
        assert_eq!(c.value, "49°C");
        let r = rows(&c.blocks[0]);
        assert_eq!(r[0], ("风扇".to_string(), "0 RPM".to_string()));
        assert_eq!(r[1], ("功耗".to_string(), "11%".to_string()));
        assert_eq!(r[2], ("显存频率".to_string(), "7.00 GHz".to_string()));
    }

    #[test]
    fn gpu_popup_shows_temperature_when_available() {
        let sensors = |c: &PopupContent| {
            c.blocks.iter().any(|b| {
                matches!(
                    b,
                    Block::Rows {
                        heading: Some("传感器"),
                        ..
                    }
                )
            })
        };
        let mut s = Snapshot::default();
        s.set(MetricKey::GpuTempC, 55.0);
        assert!(sensors(&build(
            ItemKind::Gpu,
            &s,
            &HistoryStore::new(60),
            "",
            &[]
        )));
        let without = build(
            ItemKind::Gpu,
            &Snapshot::default(),
            &HistoryStore::new(60),
            "",
            &[],
        );
        assert!(!sensors(&without));
    }

    #[test]
    fn disk_popup_lists_chosen_drives() {
        let mut s = Snapshot::default();
        s.set(MetricKey::DriveActivePercent(b'C'), 12.0);
        s.set(MetricKey::DriveUsedPercent(b'C'), 48.0);
        let c = build(ItemKind::Disk, &s, &HistoryStore::new(60), "", b"CX");
        let drives = c
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Rows {
                    heading: Some("驱动器"),
                    rows,
                } => Some(rows.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            drives[0],
            ("C:".to_string(), "活动 12% · 已用 48%".to_string())
        );
        assert_eq!(
            drives[1],
            ("X:".to_string(), "—".to_string()),
            "a missing drive has no value"
        );
    }
}
