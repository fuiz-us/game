use std::fmt::Debug;

use dashmap::DashSet;
use itertools::Itertools;

#[derive(Clone)]
#[derive_where::derive_where(Default)]
pub struct ClashSet<K: Eq + std::hash::Hash + Clone>(DashSet<K>);

impl<K> Debug for ClashSet<K>
where
    K: Eq + std::hash::Hash + Clone + Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<K> ClashSet<K>
where
    K: Eq + std::hash::Hash + Clone,
{
    pub fn contains(&self, key: &K) -> bool {
        self.0.contains(key)
    }

    pub fn insert(&self, key: K) -> bool {
        self.0.insert(key)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn vec(&self) -> Vec<K> {
        self.0.iter().map(|x| x.key().clone()).collect_vec()
    }

    pub fn remove<Q>(&self, key: &Q) -> Option<K>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.0.remove(key)
    }
}
