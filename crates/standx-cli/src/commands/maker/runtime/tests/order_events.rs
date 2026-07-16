use super::*;

#[test]
fn apply_order_response_keeps_accepted_placement() {
    let mut projection = projection_with_pending(&["req-1"]);
    let matched = apply_order_response(
        order_response(Some("req-1"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap();
    assert!(matched);
    assert_eq!(
        projection.pending_places().len(),
        1,
        "accepted placement stays pending"
    );
    assert_eq!(projection.pending_request_count(), 0);
}

#[test]
fn order_response_correlation_failed_only_on_uncorrelated_request_ids() {
    // A matched ack is never a correlation failure, even while the runtime
    // is frozen for another reason.
    assert!(!order_response_correlation_failed(true, Some("req-1")));
    // A response whose request_id matches no pending request fails closed.
    assert!(order_response_correlation_failed(false, Some("req-1")));
    // A response without a request_id cannot be correlated or escalated.
    assert!(!order_response_correlation_failed(false, None));
}

#[test]
fn account_invalidation_with_matched_buffered_ack_reconciles_without_order_response_stop() {
    // Reproduces the shutdown that a plan-affecting account event (e.g. a
    // fill) used to trigger when the cycle had already buffered one of its
    // own order acks: the freeze targets position reconciliation, but the
    // buffered ack was wrongly read as an order-response correlation
    // failure, flipping a healthy stream unhealthy and colliding with the
    // queued cleanup target.
    let mut projection = projection_with_pending(&["req-1"]);

    let mut runtime_state = MakerState::starting();
    runtime_state.handle(MakerEvent::StartupReady);
    let cycle_token = match runtime_state.next_effect() {
        Some(MakerEffect::RunCycle(token)) => token,
        effect => panic!("expected cycle effect, got {effect:?}"),
    };

    // An invalidating account event freezes the in-flight cycle and queues
    // AbortInFlight + Cleanup { PositionReconciliation }.
    runtime_state.handle(MakerEvent::CycleInvalidated {
        reason: "account state changed during maker cycle".to_string(),
    });

    // The cycle's own placement ack was buffered before the freeze and is
    // now drained. It correlates with the pending request, so it matches.
    let health = OrderResponseHealth::default();
    let response = order_response(Some("req-1"), 0);
    let request_id = response.request_id.clone();
    let matched = apply_order_response(
        response,
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap();
    assert!(matched, "buffered ack correlates with the pending request");
    if order_response_correlation_failed(matched, request_id.as_deref()) {
        health.mark_unhealthy("order-response correlation failed closed");
    }

    // A matched ack must leave the order-response stream healthy; otherwise
    // the top-of-loop health check would demand an OrderResponse cleanup.
    assert!(
        health.is_healthy(),
        "a matched ack must not flip the order-response stream unhealthy"
    );

    // The queued cleanup targets position reconciliation, so the maker
    // cleans up and can recover instead of stopping.
    take_cleanup_effect(&mut runtime_state, RecoveryTarget::PositionReconciliation)
        .expect("invalidation must drive a position-reconciliation cleanup, not a stop");

    // Stale completion of the aborted cycle is ignored; the maker stays
    // frozen awaiting recovery rather than resuming on stale work.
    runtime_state.handle(MakerEvent::CycleCompleted(cycle_token));
    assert!(runtime_state.pending_effect().is_none());
}

#[test]
fn order_response_cleanup_drain_rejects_position_reconciliation_target() {
    // Regression witness for the collision the fix removes: had a buffered
    // response been treated as an order-response fault while the runtime
    // was frozen for position reconciliation, the top-of-loop
    // order-response recovery would drain the queued cleanup with the wrong
    // target and fail closed into a stop.
    let mut runtime_state = MakerState::starting();
    runtime_state.handle(MakerEvent::StartupReady);
    let _ = runtime_state.next_effect();
    runtime_state.handle(MakerEvent::CycleInvalidated {
        reason: "account state changed during maker cycle".to_string(),
    });
    let error = take_cleanup_effect(&mut runtime_state, RecoveryTarget::OrderResponse)
        .expect_err("position-reconciliation cleanup must not satisfy an order-response drain");
    assert!(error.to_string().contains("expected OrderResponse cleanup"));
}

#[test]
fn apply_order_response_drops_rejected_placement() {
    let mut projection = projection_with_pending(&["req-1"]);
    let matched = apply_order_response(
        order_response(Some("req-1"), 1),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap();
    assert!(matched);
    assert!(
        projection.pending_places().is_empty(),
        "rejected placement is removed"
    );
}

#[test]
fn apply_order_response_matches_cancel_acknowledgement() {
    let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
    projection.apply(
        1,
        AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
            request_id: "cancel-1".to_string(),
            order_id: 7,
            side: OrderSide::Buy,
            level: 0,
            price: 100.0,
            cycle: 1,
        }),
    );

    assert!(apply_order_response(
        order_response(Some("cancel-1"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap());
    assert!(projection.pending_cancels().is_empty());
}

#[test]
fn duplicate_place_ack_matches_completed_request_after_cleanup() {
    let mut projection = projection_with_pending(&["req-1"]);
    assert!(apply_order_response(
        order_response(Some("req-1"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap());
    projection.clear_orders_and_pending();

    assert!(apply_order_response(
        order_response(Some("req-1"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        2,
        2,
    )
    .unwrap());
}

#[test]
fn delayed_account_order_and_replayed_ack_survive_account_reconnect() {
    let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
    projection.apply(
        1,
        AccountProjectionEvent::PlaceSubmitted(ProjectionPendingPlace {
            request_id: "req-1".to_string(),
            client_order_id: "sxmk-test-q00000001b0".to_string(),
            side: OrderSide::Buy,
            price: 100.0,
            qty: 1.0,
            level: 0,
            ref_center: 100.0,
            cycle: 1,
        }),
    );
    assert!(apply_order_response(
        order_response(Some("req-1"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap());
    projection.apply(1, AccountProjectionEvent::AdvanceCycle { cycle: 4 });
    projection.reset_after_cleanup_preserving_pending_acks(2, 0.0);

    let outcome = projection.apply(
        2,
        AccountProjectionEvent::OrderObserved(OrderObservation {
            order_id: 7,
            client_order_id: Some("sxmk-test-q00000001b0".to_string()),
            side: OrderSide::Buy,
            price: 100.0,
            open_qty: 1.0,
            terminal: false,
        }),
    );
    assert!(!outcome.unknown_current_run_order);
    assert!(apply_order_response(
        order_response(Some("req-1"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        4,
        2,
    )
    .unwrap());
}

#[test]
fn duplicate_place_rejection_matches_completed_request_after_cleanup() {
    let mut projection = projection_with_pending(&["req-1"]);
    assert!(apply_order_response(
        order_response(Some("req-1"), 400),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap());
    projection.clear_orders_and_pending();

    assert!(apply_order_response(
        order_response(Some("req-1"), 400),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        2,
        2,
    )
    .unwrap());
}

#[test]
fn duplicate_cancel_ack_matches_completed_request_after_cleanup() {
    let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
    projection.apply(
        1,
        AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
            request_id: "cancel-1".to_string(),
            order_id: 7,
            side: OrderSide::Buy,
            level: 0,
            price: 100.0,
            cycle: 1,
        }),
    );
    assert!(apply_order_response(
        order_response(Some("cancel-1"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap());
    projection.clear_orders_and_pending();

    assert!(apply_order_response(
        order_response(Some("cancel-1"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        2,
        2,
    )
    .unwrap());
}

#[test]
fn contradictory_replay_for_completed_request_remains_fail_closed() {
    let mut projection = projection_with_pending(&["req-1"]);
    assert!(apply_order_response(
        order_response(Some("req-1"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap());

    assert!(!apply_order_response(
        order_response(Some("req-1"), 400),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        2,
        2,
    )
    .unwrap());
}

#[test]
fn apply_order_response_fails_closed_on_rejected_cancel_acknowledgement() {
    let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
    projection.apply(
        1,
        AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
            request_id: "cancel-1".to_string(),
            order_id: 7,
            side: OrderSide::Buy,
            level: 0,
            price: 100.0,
            cycle: 1,
        }),
    );

    assert_eq!(
        apply_order_response(
            order_response(Some("cancel-1"), 400),
            &mut projection,
            OutputFormat::Quiet,
            "BTC-USD",
            1,
            2,
        ),
        Err(CancelRejection {
            request_id: "cancel-1".to_string(),
            code: 400,
            message: String::new(),
        })
    );
    assert_eq!(projection.pending_cancels().len(), 1);
    assert_eq!(projection.pending_request_count(), 1);
}

#[test]
fn apply_order_response_matches_late_ack_after_terminal_account_order() {
    let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
    projection.apply(
        1,
        AccountProjectionEvent::PlaceSubmitted(ProjectionPendingPlace {
            request_id: "req-1".to_string(),
            client_order_id: "sxmk-test-q00000001b0".to_string(),
            side: OrderSide::Buy,
            price: 100.0,
            qty: 1.0,
            level: 0,
            ref_center: 100.0,
            cycle: 1,
        }),
    );
    projection.apply(
        1,
        AccountProjectionEvent::OrderObserved(OrderObservation {
            order_id: 7,
            client_order_id: Some("sxmk-test-q00000001b0".to_string()),
            side: OrderSide::Buy,
            price: 100.0,
            open_qty: 0.0,
            terminal: true,
        }),
    );
    assert!(projection.pending_places().is_empty());
    assert_eq!(projection.pending_request_count(), 1);

    assert!(apply_order_response(
        order_response(Some("req-1"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap());
    assert_eq!(projection.pending_request_count(), 0);
}

#[test]
fn apply_order_response_reports_unmatched_ids() {
    let mut projection = projection_with_pending(&["req-1"]);
    assert!(!apply_order_response(
        order_response(Some("other"), 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap());
    assert!(!apply_order_response(
        order_response(None, 0),
        &mut projection,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap());
    assert_eq!(projection.pending_places().len(), 1);
}

#[test]
fn apply_order_responses_matched_acks_clear_request_registry() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let mut projection = projection_with_pending(&["req-1", "req-2"]);
    let mut runtime_state = MakerState::starting();
    runtime_state.handle(MakerEvent::StartupReady);
    assert!(matches!(
        runtime_state.next_effect(),
        Some(MakerEffect::RunCycle(_))
    ));

    // Benign matched acknowledgements for placements we are tracking.
    tx.try_send(order_response(Some("req-1"), 0)).unwrap();
    tx.try_send(order_response(Some("req-2"), 0)).unwrap();

    apply_order_responses(
        &mut rx,
        &mut projection,
        &mut runtime_state,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .expect("benign matched acks must not fail closed");

    assert!(runtime_state.pending_effect().is_none());
    // Accepted placements remain pending; the matched arm keeps them.
    assert_eq!(projection.pending_places().len(), 2);
    assert_eq!(projection.pending_request_count(), 0);
}

#[test]
fn apply_order_responses_unknown_request_fails_closed() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let mut projection = projection_with_pending(&[]);
    let mut runtime_state = MakerState::starting();
    runtime_state.handle(MakerEvent::StartupReady);
    assert!(matches!(
        runtime_state.next_effect(),
        Some(MakerEffect::RunCycle(_))
    ));

    tx.try_send(order_response(Some("req-1"), 0)).unwrap();
    let error = apply_order_responses(
        &mut rx,
        &mut projection,
        &mut runtime_state,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap_err();
    assert!(error.to_string().contains("correlation failed closed"));
    assert!(error.to_string().contains("request_id=req-1"));
    assert!(matches!(
        runtime_state.pending_effect(),
        Some(MakerEffect::AbortInFlight(_))
    ));
}

#[test]
fn apply_order_responses_rejected_cancel_fails_closed() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let mut projection = MakerAccountProjection::new(1, "sxmk-test-", 0.0, 0.005, 0.00005);
    projection.apply(
        1,
        AccountProjectionEvent::CancelSubmitted(ProjectionPendingCancel {
            request_id: "cancel-1".to_string(),
            order_id: 7,
            side: OrderSide::Buy,
            level: 0,
            price: 100.0,
            cycle: 1,
        }),
    );
    let mut runtime_state = MakerState::starting();
    runtime_state.handle(MakerEvent::StartupReady);
    assert!(matches!(
        runtime_state.next_effect(),
        Some(MakerEffect::RunCycle(_))
    ));

    tx.try_send(OrderResponse {
        code: 400,
        message: "cancel rejected".to_string(),
        request_id: Some("cancel-1".to_string()),
    })
    .unwrap();
    let error = apply_order_responses(
        &mut rx,
        &mut projection,
        &mut runtime_state,
        OutputFormat::Quiet,
        "BTC-USD",
        1,
        2,
    )
    .unwrap_err();

    assert!(error.to_string().contains("cancel rejected"));
    assert!(matches!(
        runtime_state.pending_effect(),
        Some(MakerEffect::AbortInFlight(_))
    ));
    assert_eq!(projection.pending_cancels().len(), 1);
}
