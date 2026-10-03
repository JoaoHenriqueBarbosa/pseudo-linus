//! (d) `intrusive_collections::intrusive_adapter!`, que expande pra `unsafe impl Send`, `unsafe impl
//! Sync`, `unsafe impl Adapter` com `unsafe fn` dentro, e põe `#[allow(unsafe_code)]` no impl.

use intrusive_collections::{LinkedList, LinkedListLink, intrusive_adapter};

pub struct Node {
    link: LinkedListLink,
    pub value: u32,
}

intrusive_adapter!(pub NodeAdapter = Box<Node>: Node { link => LinkedListLink });

pub fn sum(values: &[u32]) -> u32 {
    let mut list = LinkedList::new(NodeAdapter::new());
    for &value in values {
        list.push_back(Box::new(Node { link: LinkedListLink::new(), value }));
    }
    list.iter().map(|n| n.value).sum()
}
