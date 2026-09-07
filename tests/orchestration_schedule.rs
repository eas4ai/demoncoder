use demoncoder::subagents::schedule::{self, Gate, Node, Status};

fn node(id: u64, dependencies: &[u64], status: Status) -> Node {
    Node {
        id,
        dependencies: dependencies.to_vec(),
        status,
    }
}

#[test]
fn gates_wait_for_integration_and_admits_independent_work() {
    let nodes = vec![
        node(1, &[], Status::AwaitingIntegration),
        node(2, &[1], Status::Queued),
        node(3, &[], Status::Queued),
    ];

    assert_eq!(schedule::gate(2, &nodes).unwrap(), Gate::Waiting(vec![1]));
    assert_eq!(schedule::admit_ready(&nodes, 2).unwrap(), vec![3]);
}

#[test]
fn integration_releases_dependents_and_blocking_propagates_to_the_gate() {
    let mut nodes = vec![
        node(1, &[], Status::Integrated),
        node(2, &[1], Status::Queued),
        node(3, &[], Status::Queued),
    ];
    assert_eq!(schedule::gate(2, &nodes).unwrap(), Gate::Eligible);
    assert_eq!(schedule::admit_ready(&nodes, 2).unwrap(), vec![2, 3]);

    nodes[0].status = Status::Blocked;
    assert_eq!(schedule::gate(2, &nodes).unwrap(), Gate::Blocked(vec![1]));
    assert_eq!(schedule::admit_ready(&nodes, 2).unwrap(), vec![3]);
}

#[test]
fn gates_report_sorted_unmet_and_blocked_prerequisites() {
    let nodes = vec![
        node(1, &[], Status::AwaitingIntegration),
        node(2, &[], Status::Blocked),
        node(3, &[2, 1], Status::Queued),
    ];

    assert_eq!(schedule::gate(3, &nodes).unwrap(), Gate::Blocked(vec![2]));

    let nodes = vec![
        node(1, &[], Status::AwaitingIntegration),
        node(2, &[], Status::Active),
        node(3, &[2, 1], Status::Queued),
    ];
    assert_eq!(
        schedule::gate(3, &nodes).unwrap(),
        Gate::Waiting(vec![1, 2])
    );
}

#[test]
fn admission_counts_only_active_nodes_and_orders_queued_eligible_nodes() {
    let nodes = vec![
        node(40, &[], Status::Queued),
        node(10, &[], Status::Queued),
        node(20, &[], Status::Active),
        node(30, &[], Status::AwaitingIntegration),
        node(50, &[], Status::Blocked),
        node(60, &[], Status::Integrated),
        node(70, &[30], Status::Queued),
    ];

    assert_eq!(schedule::admit_ready(&nodes, 3).unwrap(), vec![10, 40]);
}

#[test]
fn admission_rejects_an_over_capacity_recovered_state() {
    let nodes = vec![node(1, &[], Status::Active), node(2, &[], Status::Active)];
    assert!(schedule::admit_ready(&nodes, 1).is_err());
}

#[test]
fn admission_validates_the_limit_even_for_an_empty_graph() {
    assert_eq!(schedule::admit_ready(&[], 1).unwrap(), Vec::<u64>::new());
    assert!(schedule::admit_ready(&[], 0).is_err());
    assert!(schedule::admit_ready(&[], 9).is_err());
}

#[test]
fn empty_graph_validates_and_admits_nothing() {
    assert!(schedule::validate_graph(&[]).is_ok());
    assert_eq!(schedule::admit_ready(&[], 2).unwrap(), Vec::<u64>::new());
}

#[test]
fn recovered_graphs_may_arrive_out_of_order_but_dependencies_are_lower_ids() {
    let nodes = vec![
        node(3, &[1], Status::Queued),
        node(1, &[], Status::Integrated),
        node(2, &[1], Status::Queued),
    ];

    assert!(schedule::validate_graph(&nodes).is_ok());
    assert_eq!(schedule::admit_ready(&nodes, 2).unwrap(), vec![2, 3]);
}

#[test]
fn validation_rejects_bounds_ids_duplicates_and_invalid_dependencies() {
    let too_many_nodes: Vec<_> = (1..=33).map(|id| node(id, &[], Status::Queued)).collect();
    assert!(schedule::validate_graph(&too_many_nodes).is_err());

    assert!(schedule::validate_graph(&[node(0, &[], Status::Queued)]).is_err());
    assert!(
        schedule::validate_graph(&[node(1, &[], Status::Queued), node(1, &[], Status::Queued)])
            .is_err()
    );
    assert!(
        schedule::validate_graph(&[
            node(2, &[1, 1], Status::Queued),
            node(1, &[], Status::Queued)
        ])
        .is_err()
    );
    assert!(
        schedule::validate_graph(&[node(2, &[1], Status::Queued), node(1, &[3], Status::Queued)])
            .is_err()
    );
    assert!(schedule::validate_graph(&[node(1, &[1], Status::Queued)]).is_err());
    assert!(schedule::validate_graph(&[node(2, &[9], Status::Queued)]).is_err());
    assert!(
        schedule::validate_graph(&[node(1, &[2], Status::Queued), node(2, &[], Status::Queued)])
            .is_err()
    );
}

#[test]
fn validation_rejects_more_than_31_dependencies() {
    let dependencies: Vec<_> = (1..=32).collect();
    let nodes = vec![node(33, &dependencies, Status::Queued)];
    assert!(schedule::validate_graph(&nodes).is_err());
}

#[test]
fn diamond_dependencies_require_every_direct_parent_to_be_integrated() {
    let mut nodes = vec![
        node(1, &[], Status::Integrated),
        node(2, &[1], Status::Integrated),
        node(3, &[1], Status::AwaitingIntegration),
        node(4, &[2, 3], Status::Queued),
        node(5, &[], Status::Queued),
    ];
    assert_eq!(schedule::gate(4, &nodes).unwrap(), Gate::Waiting(vec![3]));
    assert_eq!(schedule::admit_ready(&nodes, 2).unwrap(), vec![5]);

    nodes[2].status = Status::Integrated;
    assert_eq!(schedule::gate(4, &nodes).unwrap(), Gate::Eligible);
    assert_eq!(schedule::admit_ready(&nodes, 2).unwrap(), vec![4, 5]);
}

#[test]
fn unknown_gate_target_is_an_error() {
    let nodes = vec![node(1, &[], Status::Queued)];
    assert!(schedule::gate(2, &nodes).is_err());
}
