use std::fmt::Debug;

use dashmap::DashSet;

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

// impl<K> IntoIterator for ClashSet<K> where K: Eq + std::hash::Hash + Clone {
//     type IntoIter = dashmap::iter_set::OwningIter<K, std::collections::hash_map::RandomState>;
//     type Item = K;

//     fn into_iter(self) -> Self::IntoIter {
//         self.0.into_iter()
//     }
// }

pub struct Iter<'a, K, S, M> {
    inner: dashmap::iter_set::Iter<'a, K, S, M>,
}

impl<
        'a,
        K: Eq + std::hash::Hash + Clone,
        S: 'a + std::hash::BuildHasher + Clone,
        M: dashmap::Map<'a, K, (), S>,
    > Iterator for Iter<'a, K, S, M>
{
    type Item = K;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|v| v.key().to_owned())
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

    pub fn iter(
        &self,
    ) -> Iter<'_, K, std::collections::hash_map::RandomState, dashmap::DashMap<K, ()>> {
        Iter {
            inner: self.0.iter(),
        }
    }

    pub fn remove<Q>(&self, key: &Q) -> Option<K>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.0.remove(key)
    }
}
