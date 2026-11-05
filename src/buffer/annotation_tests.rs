use super::{Buffer, Edit, EditError, EditSource, LineBackground, StyledRange};
use crate::files::{DecodedFile, FileFormat};
use crate::text::ByteOffset;
use crate::theme::{StyleOverride, StyleRole};

fn buffer() -> Buffer {
    Buffer::new(
        None,
        DecodedFile {
            text: "a中\n\nlast".into(),
            format: FileFormat::default(),
        },
    )
}

fn span(start: usize, end: usize) -> StyledRange {
    StyledRange {
        range: ByteOffset(start)..ByteOffset(end),
        style: StyleOverride::new(StyleRole::Default, None, Some("#123456")).unwrap(),
    }
}

#[test]
fn annotations_are_atomic_version_bound_shared_and_do_not_modify_the_document() {
    let mut buffer = buffer();
    let shared = buffer.new_view();
    let snapshot = buffer.snapshot_id();
    let point = buffer.point();
    buffer
        .set_annotations(
            snapshot,
            vec![span(1, 4)],
            vec![LineBackground {
                rows: 0..2,
                color: gpui_kit::rgb(0x112233).into(),
            }],
        )
        .unwrap();
    assert_eq!(shared.annotations().unwrap().ranges.len(), 1);
    assert_eq!(buffer.snapshot_id(), snapshot);
    assert_eq!(buffer.point(), point);
    assert!(!buffer.is_modified());
    let annotations = shared.annotations().unwrap();
    let weak = std::sync::Arc::downgrade(&annotations);
    assert!(
        buffer
            .set_annotations(snapshot, vec![span(2, 4)], vec![])
            .is_err()
    );
    assert!(std::sync::Arc::ptr_eq(
        &annotations,
        &shared.annotations().unwrap()
    ));
    drop(annotations);
    buffer
        .edit(Edit {
            snapshot,
            range: ByteOffset(0)..ByteOffset(0),
            text: "x",
            source: EditSource::Command,
        })
        .unwrap();
    assert!(shared.annotations().is_none());
    assert!(weak.upgrade().is_none());
    assert_eq!(
        buffer.set_annotations(snapshot, vec![], vec![]),
        Err(EditError::StaleSnapshot)
    );
    assert_eq!(
        buffer.set_annotations(Buffer::scratch().snapshot_id(), vec![], vec![]),
        Err(EditError::WrongTarget)
    );
}

#[test]
fn ranges_and_lines_must_be_sorted_nonoverlapping_and_in_bounds() {
    for ranges in [
        vec![span(0, 0)],
        vec![span(0, 2)],
        vec![span(9, 20)],
        vec![span(1, 4), span(0, 1)],
        vec![span(0, 4), span(1, 4)],
    ] {
        let mut buffer = buffer();
        assert_eq!(
            buffer.set_annotations(buffer.snapshot_id(), ranges, vec![]),
            Err(EditError::InvalidRange)
        );
    }
    for rows in [0..0, 1..4, 3..4] {
        let mut buffer = buffer();
        assert_eq!(
            buffer.set_annotations(
                buffer.snapshot_id(),
                vec![],
                vec![LineBackground {
                    rows,
                    color: gpui_kit::black()
                }]
            ),
            Err(EditError::InvalidRange)
        );
    }
    let mut buffer = buffer();
    buffer
        .set_annotations(
            buffer.snapshot_id(),
            vec![span(0, 1), span(1, 4)],
            vec![LineBackground {
                rows: 0..3,
                color: gpui_kit::black(),
            }],
        )
        .unwrap();
    let annotations = buffer.annotations().unwrap();
    assert!(annotations.line_color(1).is_some());
    assert!(annotations.line_color(2).is_some());
    assert!(annotations.line_color(3).is_none());
}
