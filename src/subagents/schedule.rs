//! Pure, bounded projection of assignment dependencies and runtime states.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Queued,
    Active,
    AwaitingIntegration,
    Integrated,
    Blocked,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: u64,
    pub dependencies: Vec<u64>,
    pub status: Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate {
    Eligible,
    Waiting(Vec<u64>),
    Blocked(Vec<u64>),
}

const MAX_NODES: usize = 32;
const MAX_DEPENDENCIES: usize = 31;
const MAX_ACTIVE: usize = 8;

/// Validate the bounded dependency graph, including graphs recovered from storage.
pub fn validate_graph(nodes: &[Node]) -> Result<()> {
    ensure!(
        nodes.len() <= MAX_NODES,
        "dependency graph may contain at most {MAX_NODES} nodes"
    );

    let mut ids = BTreeSet::new();
    for node in nodes {
        ensure!(node.id != 0, "assignment IDs must be nonzero");
        ensure!(
            ids.insert(node.id),
            "assignment ID {} is duplicated",
            node.id
        );
    }

    for node in nodes {
        ensure!(
            node.dependencies.len() <= MAX_DEPENDENCIES,
            "assignment {} has more than {MAX_DEPENDENCIES} dependencies",
            node.id
        );
        let mut dependencies = BTreeSet::new();
        for dependency in &node.dependencies {
            ensure!(
                dependencies.insert(*dependency),
                "assignment {} repeats dependency {}",
                node.id,
                dependency
            );
            ensure!(
                ids.contains(dependency),
                "assignment {} depends on unknown assignment {}",
                node.id,
                dependency
            );
            ensure!(
                *dependency < node.id,
                "assignment {} depends on forward or self assignment {}",
                node.id,
                dependency
            );
        }
    }

    Ok(())
}

/// Return the dependency gate for one assignment.
pub fn gate(id: u64, nodes: &[Node]) -> Result<Gate> {
    validate_graph(nodes)?;
    let statuses: BTreeMap<_, _> = nodes.iter().map(|node| (node.id, node.status)).collect();
    ensure!(statuses.contains_key(&id), "unknown assignment {id}");

    let source = nodes
        .iter()
        .find(|node| node.id == id)
        .expect("status map contains node");
    let mut waiting = Vec::new();
    let mut blocked = Vec::new();
    for dependency in &source.dependencies {
        match statuses.get(dependency).expect("validated dependency") {
            Status::Integrated => {}
            Status::Blocked => blocked.push(*dependency),
            _ => waiting.push(*dependency),
        }
    }

    if !blocked.is_empty() {
        blocked.sort_unstable();
        Ok(Gate::Blocked(blocked))
    } else if !waiting.is_empty() {
        waiting.sort_unstable();
        Ok(Gate::Waiting(waiting))
    } else {
        Ok(Gate::Eligible)
    }
}

/// Select queued assignments that are eligible within the available capacity.
pub fn admit_ready(nodes: &[Node], limit: usize) -> Result<Vec<u64>> {
    validate_graph(nodes)?;
    ensure!(
        (1..=MAX_ACTIVE).contains(&limit),
        "active limit must be between 1 and {MAX_ACTIVE}"
    );

    let active = nodes
        .iter()
        .filter(|node| node.status == Status::Active)
        .count();
    ensure!(
        active <= limit,
        "recovered graph has {active} active assignments above limit {limit}"
    );
    let capacity = limit - active;

    let mut ready: Vec<_> = nodes
        .iter()
        .filter(|node| node.status == Status::Queued)
        .filter_map(|node| match gate(node.id, nodes) {
            Ok(Gate::Eligible) => Some(node.id),
            Ok(Gate::Waiting(_) | Gate::Blocked(_)) => None,
            Err(_) => unreachable!("graph was validated before admission"),
        })
        .collect();
    ready.sort_unstable();
    ready.truncate(capacity);
    Ok(ready)
}
