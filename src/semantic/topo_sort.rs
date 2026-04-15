use std::collections::{BTreeMap, BTreeSet};

use crate::diagnostics::{Diagnostic, DiagnosticKind};
use crate::graph::Graph;
use crate::types::{DELAY_PIECE_ID, EdgeId, GridPos};

pub(super) struct TopoAnalysis {
    pub(super) eval_order: Vec<GridPos>,
    pub(super) delay_edges: BTreeSet<EdgeId>,
    pub(super) component_members: BTreeMap<GridPos, BTreeSet<GridPos>>,
    pub(super) diagnostics: Vec<Diagnostic>,
}

pub(super) fn topo_sort_with_delay_edges(graph: &Graph) -> TopoAnalysis {
    let delay_edges: BTreeSet<EdgeId> = graph
        .edges
        .iter()
        .filter(|(_, edge)| {
            graph
                .nodes
                .get(&edge.to_node)
                .is_some_and(|node| node.piece_id == DELAY_PIECE_ID && edge.to_param == "value")
        })
        .map(|(id, _)| id.clone())
        .collect();

    let mut adjacency = BTreeMap::<GridPos, Vec<GridPos>>::new();
    let mut self_loops = BTreeSet::<GridPos>::new();
    for pos in graph.nodes.keys() {
        adjacency.insert(*pos, Vec::new());
    }

    for edge in graph.edges.values() {
        if delay_edges.contains(&edge.id) {
            continue;
        }
        if !(graph.nodes.contains_key(&edge.from) && graph.nodes.contains_key(&edge.to_node)) {
            continue;
        }
        adjacency.entry(edge.from).or_default().push(edge.to_node);
        if edge.from == edge.to_node {
            self_loops.insert(edge.from);
        }
    }

    for targets in adjacency.values_mut() {
        targets.sort();
        targets.dedup();
    }

    let mut tarjan = TarjanState::default();
    for pos in graph.nodes.keys() {
        if !tarjan.indices.contains_key(pos) {
            strong_connect(*pos, &adjacency, &mut tarjan);
        }
    }

    for component in &mut tarjan.components {
        component.sort();
    }
    tarjan.components.sort_by_key(|component| component[0]);

    let mut component_for = BTreeMap::<GridPos, usize>::new();
    for (component_id, component) in tarjan.components.iter().enumerate() {
        for pos in component {
            component_for.insert(*pos, component_id);
        }
    }

    let mut component_edges = vec![BTreeSet::<usize>::new(); tarjan.components.len()];
    let mut indegree = vec![0usize; tarjan.components.len()];
    for edge in graph.edges.values() {
        if delay_edges.contains(&edge.id) {
            continue;
        }
        let (Some(&from_component), Some(&to_component)) = (
            component_for.get(&edge.from),
            component_for.get(&edge.to_node),
        ) else {
            continue;
        };
        if from_component == to_component {
            continue;
        }
        if component_edges[from_component].insert(to_component) {
            indegree[to_component] += 1;
        }
    }

    let mut frontier = tarjan
        .components
        .iter()
        .enumerate()
        .filter_map(|(component_id, component)| {
            (indegree[component_id] == 0).then_some((component[0], component_id))
        })
        .collect::<BTreeSet<_>>();

    let mut eval_order = Vec::with_capacity(graph.nodes.len());
    while let Some(next) = frontier.iter().next().copied() {
        frontier.remove(&next);
        let component_id = next.1;
        eval_order.extend(tarjan.components[component_id].iter().copied());

        for target in component_edges[component_id].iter().copied() {
            indegree[target] = indegree[target].saturating_sub(1);
            if indegree[target] == 0 {
                frontier.insert((tarjan.components[target][0], target));
            }
        }
    }

    let diagnostics = tarjan
        .components
        .iter()
        .filter_map(|component| {
            let is_cycle =
                component.len() > 1 || component.iter().any(|pos| self_loops.contains(pos));
            is_cycle.then(|| {
                Diagnostic::error(
                    DiagnosticKind::Cycle {
                        involved: component.clone(),
                    },
                    component.first().copied(),
                )
            })
        })
        .collect();

    let mut component_members = BTreeMap::<GridPos, BTreeSet<GridPos>>::new();
    for component in &tarjan.components {
        let members = component.iter().copied().collect::<BTreeSet<_>>();
        for pos in component {
            component_members.insert(*pos, members.clone());
        }
    }

    TopoAnalysis {
        eval_order,
        delay_edges,
        component_members,
        diagnostics,
    }
}

#[derive(Default)]
struct TarjanState {
    next_index: usize,
    indices: BTreeMap<GridPos, usize>,
    lowlinks: BTreeMap<GridPos, usize>,
    stack: Vec<GridPos>,
    on_stack: BTreeSet<GridPos>,
    components: Vec<Vec<GridPos>>,
}

fn strong_connect(
    node: GridPos,
    adjacency: &BTreeMap<GridPos, Vec<GridPos>>,
    state: &mut TarjanState,
) {
    let index = state.next_index;
    state.next_index += 1;
    state.indices.insert(node, index);
    state.lowlinks.insert(node, index);
    state.stack.push(node);
    state.on_stack.insert(node);

    if let Some(targets) = adjacency.get(&node) {
        for target in targets {
            if !state.indices.contains_key(target) {
                strong_connect(*target, adjacency, state);
                let child_lowlink = *state.lowlinks.get(target).unwrap_or(&index);
                if let Some(lowlink) = state.lowlinks.get_mut(&node) {
                    *lowlink = (*lowlink).min(child_lowlink);
                }
            } else if state.on_stack.contains(target) {
                let target_index = *state.indices.get(target).unwrap_or(&index);
                if let Some(lowlink) = state.lowlinks.get_mut(&node) {
                    *lowlink = (*lowlink).min(target_index);
                }
            }
        }
    }

    let node_lowlink = *state.lowlinks.get(&node).unwrap_or(&index);
    let node_index = *state.indices.get(&node).unwrap_or(&index);
    if node_lowlink != node_index {
        return;
    }

    let mut component = Vec::new();
    while let Some(popped) = state.stack.pop() {
        state.on_stack.remove(&popped);
        component.push(popped);
        if popped == node {
            break;
        }
    }
    state.components.push(component);
}
