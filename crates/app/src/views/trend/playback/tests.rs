use super::*;
use client::history::{DirectorySnapshot, HistoryEvent, HistoryGap, LocalCoverage};
use common::history_api::{DayEntry, Eviction};
use history::{Catalog, ClockEvidence, Event, Frame, Gap};
use std::sync::Arc;

const SEALED: HistorySource = HistorySource::Sealed {
    day: 0,
    day_id: [1; 16],
    current: true,
};
const TAIL: HistorySource = HistorySource::Tail {
    generation: [2; 16],
};

fn sample(
    seq: u64,
    at: i64,
    period: u32,
    segment: u64,
    cpu: Option<u64>,
    source: HistorySource,
) -> HistorySample {
    let catalog = Arc::new(Catalog::standard(1));
    let values = catalog
        .channels
        .iter()
        .map(|column| match column.key.as_str() {
            "cpu.usage" => cpu,
            "mem.used" => Some(1),
            "mem.total" => Some(4),
            _ => None,
        })
        .collect();
    HistorySample {
        identity: SampleIdentity {
            session_id: [7; 16],
            sample_seq: seq,
        },
        frame: Frame {
            time_ms: at,
            sample_seq: seq,
            period_ms: period,
            clock: ClockEvidence::default(),
            values,
        },
        segment,
        source,
        catalog,
    }
}

fn gap(from: i64, to: i64, segment: u64, reason: GapReason) -> HistoryGap {
    HistoryGap {
        gap: Gap {
            from_ms: from,
            to_ms: to,
            segment,
            reason,
            event_id: [reason as u8; 16],
        },
        source: SEALED,
    }
}

fn show(output: &Merged, index: usize) -> Vec<(i64, i64, f32, bool)> {
    output.series[index]
        .iter()
        .map(|item| (item.from, item.to, item.value, item.break_before))
        .collect()
}

#[test]
fn historical_identity_wins_and_carry_in_respects_gap_and_channel_availability() {
    let sealed = sample(1, -200, 400, 1, Some(2_500), SEALED);
    let duplicate = sample(1, -200, 400, 1, Some(9_000), TAIL);
    let next = sample(2, 900, 400, 1, None, TAIL);
    let live = VecDeque::from([LivePoint {
        at: -200,
        identity: Some(sealed.identity),
        period: 400,
        values: [Some(1.0); 3],
        epoch: 0,
        rollback: false,
    }]);
    let history = Playback::new(
        HistoryQuery {
            samples: vec![duplicate, next, sealed],
            gaps: vec![gap(250, 900, 1, GapReason::Storage)],
            ..HistoryQuery::default()
        },
        Some((0, 2000).into()),
    );
    let merged = merge(Some(&history), &live, (0, 2000).into());
    assert_eq!(show(&merged, 0), vec![(0, 200, 0.25, true)]);
    assert_eq!(
        show(&merged, 1),
        vec![(0, 200, 0.25, true), (900, 1300, 0.25, true)]
    );
    assert!(merged.series[2].is_empty());
    assert_eq!(
        merged.bands,
        vec![Band {
            range: (250, 900).into(),
            kind: BandKind::Gap {
                reason: GapReason::Storage,
                duration: 650
            }
        }]
    );
}

#[test]
fn newer_clock_segment_overrides_older_only_where_it_has_evidence() {
    let history = Playback::new(
        HistoryQuery {
            samples: vec![
                sample(1, 0, 300, 1, Some(1_000), SEALED),
                sample(2, 300, 700, 1, Some(1_000), SEALED),
                sample(3, 200, 200, 2, Some(8_000), TAIL),
                sample(4, 650, 100, 2, Some(9_000), TAIL),
            ],
            gaps: vec![gap(400, 650, 2, GapReason::Sleep)],
            events: vec![HistoryEvent {
                event: Event {
                    kind: MarkerKind::ClockSegment,
                    event_id: [4; 16],
                    segment: 2,
                    session_ref: 1,
                    sample_seq: Some(3),
                    clock: ClockEvidence {
                        wall_ms: Some(200),
                        ..ClockEvidence::default()
                    },
                },
                source: TAIL,
                session_id: [7; 16],
            }],
            ..HistoryQuery::default()
        },
        None,
    );
    let merged = merge(Some(&history), &VecDeque::new(), (0, 1000).into());
    assert_eq!(
        show(&merged, 0),
        vec![
            (0, 200, 0.1, true),
            (200, 400, 0.8, true),
            (650, 750, 0.9, true),
            (750, 1000, 0.1, true)
        ]
    );
    assert_eq!(merged.bands[0].range, (400, 650).into());
    assert_eq!(
        merged.marks,
        vec![Mark {
            at: 200,
            rollback: true
        }]
    );
}

#[test]
fn expired_bands_yield_to_local_data_and_watermarks_only_suppress_live_in_scoped_coverage() {
    let history = Playback::new(
        HistoryQuery {
            samples: vec![sample(1, 100, 100, 1, Some(2_500), SEALED)],
            directories: vec![DirectorySnapshot {
                range: (0, 600).into(),
                age_cutoff_day: None,
                entries: vec![
                    DirectoryEntry::Evicted(Eviction {
                        day: -1,
                        day_id: [0; 16],
                        data_range: Some((0, 250).into()),
                        gap_range: None,
                    }),
                    DirectoryEntry::Available(DayEntry {
                        day: 0,
                        day_id: [1; 16],
                        published_through: 100,
                        data_range: Some((250, 450).into()),
                        gap_range: None,
                    }),
                    DirectoryEntry::Available(DayEntry {
                        day: 1,
                        day_id: [2; 16],
                        published_through: 100,
                        data_range: Some((450, 550).into()),
                        gap_range: None,
                    }),
                ],
            }],
            coverage: vec![LocalCoverage {
                day: 0,
                day_id: [1; 16],
                cached_through: 100,
                published_through: Some(100),
                data_range: Some((250, 450).into()),
                gap_range: None,
                durable: true,
                current: true,
            }],
            ..HistoryQuery::default()
        },
        Some((0, 400).into()),
    );
    let live = VecDeque::from([LivePoint {
        at: 300,
        identity: None,
        period: 125,
        values: [Some(1.0); 3],
        epoch: 0,
        rollback: false,
    }]);
    let merged = merge(Some(&history), &live, (0, 600).into());
    assert_eq!(
        show(&merged, 0),
        vec![(100, 200, 0.25, true), (400, 425, 1., true)]
    );
    assert_eq!(
        merged
            .bands
            .iter()
            .map(|band| band.range)
            .collect::<Vec<_>>(),
        vec![(0, 100).into(), (200, 250).into(), (450, 550).into()]
    );
    assert_eq!(merged.bands[2].kind, BandKind::Sync);
}
