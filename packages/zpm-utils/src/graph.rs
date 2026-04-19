use std::collections::{BTreeMap, BTreeSet};

pub fn scc_tarjan_pearce_core(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let n
        = adj.len();

    if n == 0 {
        return Vec::new();
    }

    let mut rindex
        = vec![0usize; n];

    let mut stack
        = Vec::with_capacity(n);

    let mut index
        = 1usize;

    let mut c
        = n - 1;

    let mut comps
        = Vec::new();

    fn visit(
        v: usize,
        adj: &[Vec<usize>],
        rindex: &mut [usize],
        stack: &mut Vec<usize>,
        index: &mut usize,
        c: &mut usize,
        comps: &mut Vec<Vec<usize>>,
    ) {
        let mut is_root
            = true;

        rindex[v] = *index;
        *index += 1;

        for &w in &adj[v] {
            if rindex[w] == 0 {
                visit(w, adj, rindex, stack, index, c, comps);
            }

            if rindex[w] < rindex[v] {
                rindex[v] = rindex[w];
                is_root = false;
            }
        }

        if is_root {
            *index -= 1;

            let mut comp
                = Vec::new();

            while let Some(&top) = stack.last() {
                if rindex[v] <= rindex[top] {
                    let w
                        = stack.pop().unwrap();

                    rindex[w] = *c;
                    *index -= 1;

                    comp.push(w);
                } else {
                    break;
                }
            }

            rindex[v] = *c;
            comp.push(v);

            *c = c.saturating_sub(1);

            comps.push(comp);
        } else {
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

pub fn scc_tarjan_pearce<T>(graph: &BTreeMap<T, BTreeSet<T>>) -> Vec<Vec<T>>
where
    T: Eq + Ord + Clone,
{
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

    for k in graph.keys() {
        intern(k.clone(), &mut id_of, &mut key_of);
    }

    for nbrs in graph.values() {
        for v in nbrs {
            intern(v.clone(), &mut id_of, &mut key_of);
        }
    }

    let n
        = key_of.len();

    let mut adj_idx
        = vec![Vec::new(); n];

    for (u, nbrs) in graph.iter() {
        let ui
            = id_of[u];

        for v in nbrs {
            let vi
                = id_of[v];

            adj_idx[ui].push(vi);
        }
    }

    let comps_idx
        = scc_tarjan_pearce_core(&adj_idx);

    comps_idx
        .into_iter()
        .map(|c| c.into_iter().map(|i| key_of[i].clone()).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_cycles() {
        let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        graph.insert("a", ["b", "c"].into_iter().collect());
        graph.insert("b", ["d"].into_iter().collect());
        graph.insert("c", ["d"].into_iter().collect());
        graph.insert("d", BTreeSet::new());

        let sccs = scc_tarjan_pearce(&graph);

        // All SCCs should have size 1 (no cycles)
        assert!(sccs.iter().all(|scc| scc.len() == 1));
        assert_eq!(sccs.len(), 4);
    }

    #[test]
    fn test_with_cycle() {
        let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        graph.insert("a", ["b"].into_iter().collect());
        graph.insert("b", ["c"].into_iter().collect());
        graph.insert("c", ["a"].into_iter().collect());

        let sccs = scc_tarjan_pearce(&graph);

        // Should have one SCC with all 3 nodes (the cycle)
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].len(), 3);
    }

    #[test]
    fn test_topological_order() {
        // Graph: a depends on b, b depends on c
        // a -> b -> c
        let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        graph.insert("a", ["b"].into_iter().collect());
        graph.insert("b", ["c"].into_iter().collect());
        graph.insert("c", BTreeSet::new());

        let sccs = scc_tarjan_pearce(&graph);

        // SCCs are returned in topological order (dependencies first)
        // So c comes before b, and b comes before a
        let order: Vec<&str> = sccs.iter().map(|scc| scc[0]).collect();
        let a_pos = order.iter().position(|&x| x == "a").unwrap();
        let b_pos = order.iter().position(|&x| x == "b").unwrap();
        let c_pos = order.iter().position(|&x| x == "c").unwrap();

        // Topological: c before b before a (dependencies first)
        assert!(c_pos < b_pos, "c should come before b");
        assert!(b_pos < a_pos, "b should come before a");
    }
}
