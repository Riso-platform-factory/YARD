use super::*;

#[test]
fn flexible_handle_does_not_take_space_from_fixed_successors() {
    let mut sizes = [
        AxisSize::Flex(1.),
        AxisSize::Flex(1.),
        AxisSize::Fixed(px(240.)),
    ];
    assert!(resize_sizes(&mut sizes, 0, 840., 700., 80.));
    assert_eq!(sizes[2], AxisSize::Fixed(px(240.)));
    let (lengths, _) = layout_sizes(&sizes, 840., 80.);
    for (actual, expected) in lengths.iter().zip([520., 80., 240.]) {
        assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
    }
}

#[test]
fn a_drag_of_shrunken_fixed_children_changes_only_the_adjacent_sizes() {
    let mut sizes = [
        AxisSize::Fixed(px(240.)),
        AxisSize::Fixed(px(240.)),
        AxisSize::Fixed(px(240.)),
        AxisSize::Flex(1.),
    ];
    assert_eq!(layout_sizes(&sizes, 560., 80.).0, [160., 160., 160., 80.]);
    assert!(resize_sizes(&mut sizes, 0, 560., 180., 80.));
    assert_eq!(
        sizes,
        [
            AxisSize::Fixed(px(180.)),
            AxisSize::Fixed(px(140.)),
            AxisSize::Fixed(px(240.)),
            AxisSize::Flex(1.),
        ]
    );
    assert_eq!(layout_sizes(&sizes, 800., 80.).0, [180., 140., 240., 240.]);
}

#[test]
fn dragging_a_fixed_flex_handle_preserves_other_shrunken_fixed_sizes() {
    for (mut sizes, handle, target, untouched, dragged) in [
        (
            [
                AxisSize::Fixed(px(240.)),
                AxisSize::Fixed(px(240.)),
                AxisSize::Flex(1.),
            ],
            1,
            20.,
            0,
            1,
        ),
        (
            [
                AxisSize::Flex(1.),
                AxisSize::Fixed(px(240.)),
                AxisSize::Fixed(px(240.)),
            ],
            0,
            220.,
            2,
            1,
        ),
    ] {
        assert_eq!(layout_sizes(&sizes, 400., 80.).0[untouched], 160.);
        assert!(resize_sizes(&mut sizes, handle, 400., target, 80.));
        assert_eq!(sizes[untouched], AxisSize::Fixed(px(240.)));
        assert_eq!(sizes[dragged], AxisSize::Fixed(px(80.)));
        let (lengths, _) = layout_sizes(&sizes, 800., 80.);
        assert_eq!(lengths[untouched], 240.);
        assert_eq!(lengths[dragged], 80.);
    }
}
