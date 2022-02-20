use crate::*;

const DUMMY_IDX: Index = Index::new(u64::MAX, 0);

struct GraphNode {
    incoming: Vec<Index>,
    outgoing: Vec<Index>,
    data_idx: Index,
}

impl GraphNode {
    pub(crate) fn update_data_idx(&mut self, new_index: Index) {
        self.data_idx = new_index;
    }

    pub(crate) fn add_incoming(&mut self, incoming_node: Index) {
        if !self.incoming.contains(&incoming_node) {
            self.incoming.push(incoming_node);
            if self.incoming.len() > 1 {
                self.incoming.sort_unstable();
            }
        }
    }

    pub(crate) fn add_outgoing(&mut self, outgoing_node: Index) {
        if !self.outgoing.contains(&outgoing_node) {
            self.outgoing.push(outgoing_node);
            if self.outgoing.len() > 1 {
                self.outgoing.sort_unstable();
            }
        }
    }
}

#[allow(dead_code)]
struct DataNode<T> {
    payload: T,
    graph_idx: Index,
}

impl<T> DataNode<T> {
    #[allow(unused)]
    pub(crate) fn update_graph_idx(&mut self, new_index: Index) {
        self.graph_idx = new_index;
    }
}

struct Graph<T> {
    structure: Pool<GraphNode>,
    data: Pool<DataNode<T>>,
}

impl<T> Graph<T> {
    #[allow(unused)]
    pub fn new() -> Self {
        Graph {
            structure: Pool::new(),
            data: Pool::new(),
        }
    }

    #[allow(unused)]
    pub fn add_new_node(&mut self, data: T) -> Index {
        let graph_node = GraphNode {
            incoming: Vec::new(),
            outgoing: Vec::new(),
            data_idx: DUMMY_IDX,
        };
        let graph_index = self.structure.add(graph_node);
        let data_node = DataNode {
            payload: data,
            graph_idx: graph_index,
        };
        let data_index = self.data.add(data_node);
        self.structure
            .get_mut(graph_index)
            .unwrap()
            .update_data_idx(data_index);
        data_index
    }

    #[allow(unused)]
    pub fn link_nodes(&mut self, from: Index, to: Index) -> bool {
        if let Some(DataNode { graph_idx, .. }) = self.data.get(from) {
            let from_graph_idx = graph_idx;
            if let Some(DataNode { graph_idx, .. }) = self.data.get(to) {
                let to_graph_idx = graph_idx;
                self.structure
                    .get_mut(*from_graph_idx)
                    .unwrap()
                    .add_outgoing(*to_graph_idx);
                self.structure
                    .get_mut(*to_graph_idx)
                    .unwrap()
                    .add_incoming(*from_graph_idx);
                return true;
            }
        }
        false
    }
}
