use std::collections::{BTreeMap, BTreeSet};

/// Strongly Connected Components using the Tarjan–Pearce (space-efficient) variant.
/// Input: adjacency list for vertices 0..n-1
/// Output: Vec of components, each a Vec<usize>, in reverse topological order.
pub fn scc_tarjan_pearce_core(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n = adj.len();
    if n == 0 {
        return Vec::new();
    }

    // rindex[v] = 0           => unvisited
    // rindex[v] in [1..index) => DFS "rindex" = index of v's current local root (active)
    // rindex[v] >= c_min      => vertex has been assigned to a component with id rindex[v]
    let mut rindex
        = vec![0usize; n];

    // Stack of vertices that are candidates for the current SCC.
    let mut stack
        = Vec::with_capacity(n);

    // DFS visit counter (starts at 1 in Pearce’s Alg. 3).
    let mut index
        = 1usize;

    // Component id (starts at n-1 and decrements) – ensures index < c always holds.
    let mut c
        = n - 1;

    // Result components, collected when a root is closed.
    let mut comps
        = Vec::new();

    // Recursive DFS as in Pearce’s Algorithm 3 (PEA_FIND_SCC2).
    fn visit(
        v: usize,
        adj: &[Vec<usize>],
        rindex: &mut [usize],
        stack: &mut Vec<usize>,
        index: &mut usize,
        c: &mut usize,
        comps: &mut Vec<Vec<usize>>,
    ) {
        let mut is_root = true;

        // Enter v: set its initial rindex to current index and advance.
        rindex[v] = *index;
        *index += 1;

        // Explore all out-edges (v -> w).
        for &w in &adj[v] {
            if rindex[w] == 0 {
                // Tree edge to an unvisited node.
                visit(w, adj, rindex, stack, index, c, comps);
            }

            // If w is not yet assigned to a component, its rindex is < current c,
            // so a smaller rindex[w] indicates a better (earlier) local root.
            if rindex[w] < rindex[v] {
                rindex[v] = rindex[w];
                is_root = false;
            }
        }

        if is_root {
            // v is the root of its SCC: finalize all vertices whose rindex >= rindex[v]
            // (including v itself) — they form one component.
            *index -= 1;

            let mut comp = Vec::new();

            while let Some(&top) = stack.last() {
                if rindex[v] <= rindex[top] {
                    let w
                        = stack.pop().unwrap();

                    rindex[w] = *c; // assign component id
                    *index -= 1;    // Pearce decrements index for each assignment

                    comp.push(w);
                } else {
                    break;
                }
            }

            // Assign v itself.
            rindex[v] = *c;
            comp.push(v);

            // Decrement component id for the next SCC.
            *c = c.saturating_sub(1);

            // We produced the SCC for root v.
            comps.push(comp);
        } else {
            // Not a root — stay on the candidate stack.
            stack.push(v);
        }
    }

    for v in 0..n {
        if rindex[v] == 0 {
            visit(v, adj, &mut rindex, &mut stack, &mut index, &mut c, &mut comps);
        }
    }

    comps
}

pub fn scc_tarjan_pearce<T>(graph: &BTreeMap<T, BTreeSet<T>>) -> Vec<Vec<T>> where T: Eq + Ord + Clone {
    // 1) Assign dense indices to *all* vertices (keys and neighbors).
    let mut id_of
        = BTreeMap::new();
    let mut key_of
        = Vec::new();

    let intern = |x: T, id_of: &mut BTreeMap<T, usize>, key_of: &mut Vec<T>| -> usize {
        if let Some(&i) = id_of.get(&x) {
            i
        } else {
            let i
                = key_of.len();

            key_of.push(x.clone());
            id_of.insert(x, i);

            i
        }
    };

    // Include keys.
    for k in graph.keys() {
        intern(k.clone(), &mut id_of, &mut key_of);
    }

    // Include neighbors that might not appear as keys.
    for nbrs in graph.values() {
        for v in nbrs {
            intern(v.clone(), &mut id_of, &mut key_of);
        }
    }

    // 2) Build the indexed adjacency list.
    let n
        = key_of.len();
    let mut adj_idx
        = vec![Vec::new(); n];

    for (u, nbrs) in graph.iter() {
        let ui
            = id_of[u];

        for v in nbrs {
            let vi = id_of[v];
            adj_idx[ui].push(vi);
        }
    }

    // 3) Run core SCC on indices and map components back to T.
    let comps_idx
        = scc_tarjan_pearce_core(&adj_idx);

    comps_idx
        .into_iter()
        .map(|c| c.into_iter().map(|i| key_of[i].clone()).collect())
        .collect()
}
