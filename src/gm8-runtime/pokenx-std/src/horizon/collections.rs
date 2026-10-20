//! `std::collections` over `alloc` and `hashbrown`. The hasher that `std::HashMap`
//! uses by default needs OS randomness, so `RandomState` maps to hashbrown's default.

pub use alloc_crate::collections::{
    binary_heap, btree_map, btree_set, linked_list, vec_deque, BTreeMap, BTreeSet, BinaryHeap, LinkedList, TryReserveError,
    VecDeque,
};
pub use hashbrown::{HashMap, HashSet};

pub mod hash_map {
    pub use hashbrown::hash_map::*;
    pub use hashbrown::DefaultHashBuilder as RandomState;
}

pub mod hash_set {
    pub use hashbrown::hash_set::*;
}
