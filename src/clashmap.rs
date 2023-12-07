use std::fmt::Debug;

use dashmap::{mapref::entry::Entry, DashMap};

#[derive(Clone)]
#[derive_where::derive_where(Default)]
pub struct ClashMap<K: Eq + std::hash::Hash + Clone, V: Clone>(DashMap<K, V>);

impl<K, V> Debug for ClashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone + Debug,
    V: Clone + Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

pub struct Iter<'a, K, V, S, M> {
    inner: dashmap::iter::Iter<'a, K, V, S, M>,
}

impl<
        'a,
        K: Eq + std::hash::Hash + Clone,
        V: Clone,
        S: 'a + std::hash::BuildHasher + Clone,
        M: dashmap::Map<'a, K, V, S>,
    > Iterator for Iter<'a, K, V, S, M>
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|v| {
            let (k, v) = v.pair();
            (k.to_owned(), v.to_owned())
        })
    }
}

impl<K, V> ClashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone,
{
    pub fn get(&self, key: &K) -> Option<V> {
        self.0.get(key).map(|v| v.to_owned())
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.0.contains_key(key)
    }

    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.0.insert(key, value)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn remove(&self, key: &K) -> Option<(K, V)> {
        self.0.remove(key)
    }

    pub fn iter(&self) -> Iter<'_, K, V, std::collections::hash_map::RandomState, DashMap<K, V>> {
        Iter {
            inner: self.0.iter(),
        }
    }

    pub fn insert_if_vacant(&self, key: K, value: V) -> Option<(K, V)> {
        match self.0.entry(key) {
            Entry::Occupied(o) => Some((o.into_key(), value)),
            Entry::Vacant(v) => {
                v.insert(value);
                None
            }
        }
    }
}

impl<K, V> ClashMap<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Clone + Default,
{
    pub fn modify_entry_or_default<F>(&self, key: K, f: F)
    where
        F: Fn(&mut V),
    {
        match self.0.entry(key) {
            Entry::Occupied(mut o) => f(o.get_mut()),
            Entry::Vacant(v) => {
                let mut value = V::default();
                f(&mut value);
                v.insert(value);
            }
        }
    }
}
