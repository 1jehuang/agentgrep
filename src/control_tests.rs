use super::*;
use std::hint::black_box;

fn control(limits: SearchLimits) -> SearchControl {
    SearchControl::new(Arc::new(AtomicBool::new(false)), limits)
}

#[test]
fn nested_scoped_closures_restore_on_return_error_and_unwind() {
    assert!(current().is_none());
    let outer = control(SearchLimits {
        max_entries: 7,
        ..SearchLimits::default()
    });
    let outer_usage = outer.usage.clone();
    with_search_control(outer, || {
        scoped(|| {
            with_search_control(control(SearchLimits::default()), || {
                assert!(!Arc::ptr_eq(&current().unwrap().usage, &outer_usage));
            })
            .unwrap();
            assert!(Arc::ptr_eq(&current().unwrap().usage, &outer_usage));
            let panic = std::panic::catch_unwind(|| {
                let _ = with_search_control(control(SearchLimits::default()), || panic!("unwind"));
            });
            assert!(panic.is_err());
            assert!(Arc::ptr_eq(&current().unwrap().usage, &outer_usage));
            let canceled = control(SearchLimits::default());
            canceled.token.store(true, Ordering::Relaxed);
            assert!(with_search_control(canceled, || panic!("must not run")).is_err());
            assert_eq!(limits().max_entries, 7);
            Ok(())
        })
        .unwrap();
    })
    .unwrap();
    assert!(current().is_none());
    scoped(|| with_search_control(control(SearchLimits::default()), || ()).map(|_| ())).unwrap();
    assert!(current().is_none());
}

#[test]
fn inherited_worker_consumes_shared_budget_and_first_error() {
    let c = control(SearchLimits {
        max_entries: 2,
        ..SearchLimits::default()
    });
    let shared = c.clone();
    let error = with_search_control(c, || {
        charge("entries", 1).unwrap();
        let inherited_control = current();
        std::thread::spawn(move || {
            inherited(inherited_control, || {
                charge("entries", 1).unwrap();
                charge("entries", 1).unwrap_err()
            })
        })
        .join()
        .unwrap()
    })
    .unwrap_err();
    assert_eq!(error, "search resource limit exceeded: entries (limit 2)");
    assert_eq!(shared.usage.lock().unwrap().entries, 3);
    assert_eq!(shared.usage.lock().unwrap().error.as_ref(), Some(&error));
    assert!(current().is_none());
}

#[test]
fn old_and_new_error_precedence_and_counters_match() {
    for old in [true, false] {
        let check = if old {
            baseline::checkpoint
        } else {
            checkpoint
        };
        let charge_fn = if old { baseline::charge } else { charge };
        let fail_fn = |s: &str| if old { baseline::fail(s) } else { fail(s) };
        let c = control(SearchLimits {
            max_entries: 0,
            ..SearchLimits::default()
        });
        let shared = c.clone();
        let result = with_search_control(c, || {
            let resource = charge_fn("entries", 1).unwrap_err();
            shared.token.store(true, Ordering::Relaxed);
            assert_eq!(check().unwrap_err(), resource);
            assert_eq!(fail_fn("later"), resource);
            assert_eq!(charge_fn("entries", 99).unwrap_err(), resource);
        });
        assert!(result.is_err());
        assert_eq!(shared.usage.lock().unwrap().entries, 1);

        // Install directly to exercise simultaneous expired/canceled state at checkpoint.
        let c = control(SearchLimits {
            timeout: Duration::ZERO,
            ..SearchLimits::default()
        });
        c.token.store(true, Ordering::Relaxed);
        let previous = CONTROL.with(|slot| slot.replace(Some(c)));
        let restore = Restore(previous);
        assert_eq!(check().unwrap_err(), "search cancelled");
        drop(restore);
        let c = control(SearchLimits {
            timeout: Duration::ZERO,
            ..SearchLimits::default()
        });
        let previous = CONTROL.with(|slot| slot.replace(Some(c)));
        let restore = Restore(previous);
        assert_eq!(check().unwrap_err(), "search deadline exceeded");
        drop(restore);
    }
}

#[test]
fn message_conversion_can_reenter_and_no_scope_operations_remain_unlimited() {
    struct Reenter;
    impl From<Reenter> for String {
        fn from(_: Reenter) -> Self {
            with_search_control(control(SearchLimits::default()), || {
                checkpoint().unwrap();
            })
            .unwrap();
            "converted".to_owned()
        }
    }
    assert!(current().is_none());
    charge("entries", usize::MAX).unwrap();
    assert_eq!(fail(Reenter), "converted");
    assert!(current().is_none());
    let c = control(SearchLimits::default());
    assert_eq!(
        with_search_control(c, || fail(Reenter)).unwrap_err(),
        "converted"
    );
    assert!(current().is_none());
}

#[test]
fn every_counter_saturates_and_preserves_boundary() {
    for kind in ["read bytes", "entries", "structure items", "matches"] {
        for old in [true, false] {
            let charge_fn = if old { baseline::charge } else { charge };
            let c = control(SearchLimits {
                max_total_read_bytes: 1,
                max_entries: 1,
                max_structure_items: 1,
                max_matches: 1,
                ..SearchLimits::default()
            });
            let expected = format!("search resource limit exceeded: {kind} (limit 1)");
            assert_eq!(
                with_search_control(c, || {
                    charge_fn(kind, 1).unwrap();
                    assert_eq!(charge_fn(kind, 1).unwrap_err(), expected);
                })
                .unwrap_err(),
                expected
            );
            let c = control(SearchLimits {
                max_total_read_bytes: usize::MAX,
                max_entries: usize::MAX,
                max_structure_items: usize::MAX,
                max_matches: usize::MAX,
                ..SearchLimits::default()
            });
            with_search_control(c, || {
                charge_fn(kind, usize::MAX).unwrap();
                charge_fn(kind, 1).unwrap();
            })
            .unwrap();
        }
    }
}

#[test]
#[ignore = "paired component benchmark, run explicitly with --ignored --nocapture"]
fn paired_control_benchmark() {
    assert!(
        black_box(cfg!(debug_assertions)),
        "measure unchanged default dev/test optimization"
    );
    const N: usize = 500_000;
    for round in 0..6 {
        for old in if round % 2 == 0 {
            [true, false]
        } else {
            [false, true]
        } {
            let c = control(SearchLimits {
                timeout: Duration::from_secs(300),
                ..SearchLimits::default()
            });
            with_search_control(c, || {
                let start = Instant::now();
                for _ in 0..N {
                    if old {
                        baseline::checkpoint().unwrap();
                        baseline::charge(black_box("matches"), black_box(0)).unwrap();
                        black_box(baseline::limits());
                    } else {
                        checkpoint().unwrap();
                        charge(black_box("matches"), black_box(0)).unwrap();
                        black_box(limits());
                    }
                }
                eprintln!(
                    "paired_control round={round} old={old} iterations={N} elapsed_ns={}",
                    start.elapsed().as_nanos()
                );
            })
            .unwrap();

            let c = control(SearchLimits::default());
            let previous = CONTROL.with(|slot| slot.replace(Some(c)));
            let restore = Restore(previous);
            let start = Instant::now();
            for _ in 0..N {
                if old {
                    black_box(baseline::fail(black_box("first error")));
                } else {
                    black_box(fail(black_box("first error")));
                }
            }
            eprintln!(
                "paired_fail round={round} old={old} iterations={N} elapsed_ns={}",
                start.elapsed().as_nanos()
            );
            drop(restore);
        }
    }
}
