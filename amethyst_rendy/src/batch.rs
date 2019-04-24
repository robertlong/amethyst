use crate::util::TapCountIter;
use derivative::Derivative;
use smallvec::{smallvec, SmallVec};
use std::{
    collections::hash_map::Entry,
    iter::{Extend, FromIterator},
};

pub trait GroupIterator<K, V>
where
    Self: Iterator<Item = (K, V)> + Sized,
    K: PartialEq,
{
    /// Perform grouping. Evaluates passed closure on every next
    /// countiguous list of data with same group identifier.
    fn for_each_group<F>(self, on_group: F)
    where
        F: FnMut(K, &mut Vec<V>);
}

// This would be an iterator adaptor if `Item` type would allow a borrow on iterator itself.
// FIXME: Implement once `StreamingIterator` is a thing.
impl<K, V, I> GroupIterator<K, V> for I
where
    K: PartialEq,
    I: Iterator<Item = (K, V)>,
{
    fn for_each_group<F>(self, mut on_group: F)
    where
        F: FnMut(K, &mut Vec<V>),
    {
        let mut block: Option<(K, Vec<V>)> = None;

        for (next_group_id, value) in self {
            match &mut block {
                slot @ None => {
                    let mut group_buffer = Vec::with_capacity(64);
                    group_buffer.push(value);
                    slot.replace((next_group_id, group_buffer));
                }
                Some((group_id, group_buffer)) if group_id == &next_group_id => {
                    group_buffer.push(value);
                }
                Some((group_id, ref mut group_buffer)) => {
                    let submitted_group_id = std::mem::replace(group_id, next_group_id);
                    on_group(submitted_group_id, group_buffer);
                    group_buffer.clear();
                    group_buffer.push(value);
                }
            }
        }

        if let Some((group_id, mut group_buffer)) = block.take() {
            on_group(group_id, &mut group_buffer);
        }
    }
}

#[derive(Debug)]
pub struct BatchData<K, C> {
    pub key: K,
    pub collection: C,
}

pub trait BatchType {
    type Key: PartialEq;
    type Data;
    fn key(&self) -> &Self::Key;
    fn extend(&mut self, vals: impl IntoIterator<Item = Self::Data>);
    fn new(key: Self::Key, vals: impl IntoIterator<Item = Self::Data>) -> Self;
}

impl<K, C> BatchType for BatchData<K, C>
where
    K: PartialEq,
    C: IntoIterator,
    C: FromIterator<<C as IntoIterator>::Item>,
    C: Extend<<C as IntoIterator>::Item>,
{
    type Key = K;
    type Data = C::Item;
    fn key(&self) -> &Self::Key {
        &self.key
    }
    fn extend(&mut self, vals: impl IntoIterator<Item = Self::Data>) {
        &self.collection.extend(vals);
    }
    fn new(key: Self::Key, vals: impl IntoIterator<Item = Self::Data>) -> Self {
        BatchData {
            key,
            collection: vals.into_iter().collect(),
        }
    }
}

pub trait BatchPrimitives {
    type Shell;
    type Batch: BatchType;

    fn wrap_batch(batch: Self::Batch) -> Self::Shell;
    fn push(shell: &mut Self::Shell, batch: Self::Batch);
    fn batches_mut(shell: &mut Self::Shell) -> &mut [Self::Batch];

    fn insert_batch<
        K: std::hash::Hash + PartialEq,
        I: IntoIterator<Item = <Self::Batch as BatchType>::Data>,
    >(
        entry: Entry<'_, K, Self::Shell>,
        batch_key: <Self::Batch as BatchType>::Key,
        instance_data: I,
    ) {
        match entry {
            Entry::Occupied(mut e) => {
                let shell = e.get_mut();

                // scan for the same key to try to combine batches.
                // Scanning up to next 8 slots to limit complexity.
                if let Some(batch) = Self::batches_mut(shell)
                    .iter_mut()
                    .take(8)
                    .find(|b| b.key() == &batch_key)
                {
                    batch.extend(instance_data);
                    return;
                }
                Self::push(shell, Self::Batch::new(batch_key, instance_data));
            }
            Entry::Vacant(e) => {
                e.insert(Self::wrap_batch(Self::Batch::new(batch_key, instance_data)));
            }
        }
    }
}

#[derive(Derivative, Debug)]
#[derivative(Default(bound = ""))]
pub struct TwoLevelBatch<PK, SK, C>
where
    PK: Eq + std::hash::Hash,
{
    map: fnv::FnvHashMap<PK, SmallVec<[BatchData<SK, C>; 1]>>,
    data_count: usize,
}

impl<PK, SK, C> TwoLevelBatch<PK, SK, C>
where
    PK: Eq + std::hash::Hash,
    SK: PartialEq,
    C: IntoIterator,
    C: FromIterator<<C as IntoIterator>::Item>,
    C: Extend<<C as IntoIterator>::Item>,
{
    pub fn clear_inner(&mut self) {
        self.data_count = 0;
        for (_, data) in self.map.iter_mut() {
            data.clear();
        }
    }

    pub fn prune(&mut self) {
        self.map.retain(|_, b| b.len() > 0);
    }

    pub fn insert(&mut self, pk: PK, sk: SK, data: impl IntoIterator<Item = C::Item>) {
        Self::insert_batch(
            self.map.entry(pk),
            sk,
            data.into_iter().tap_count(&mut self.data_count),
        );
    }

    pub fn data<'a>(&'a self) -> impl Iterator<Item = &'a C> {
        self.map
            .iter()
            .flat_map(|(_, batch)| batch.iter().map(|data| &data.collection))
    }

    pub fn iter<'a>(
        &'a self,
    ) -> impl Iterator<Item = (&'a PK, impl Iterator<Item = (&'a SK, &'a C)>)> {
        self.map
            .iter()
            .map(|(pk, batch)| (pk, batch.iter().map(|data| (&data.key, &data.collection))))
    }

    pub fn count(&self) -> usize {
        self.data_count
    }
}

impl<PK, SK, C> BatchPrimitives for TwoLevelBatch<PK, SK, C>
where
    PK: Eq + std::hash::Hash,
    SK: PartialEq,
    C: IntoIterator,
    C: FromIterator<<C as IntoIterator>::Item>,
    C: Extend<<C as IntoIterator>::Item>,
{
    type Shell = SmallVec<[BatchData<SK, C>; 1]>;
    type Batch = BatchData<SK, C>;

    fn wrap_batch(batch: Self::Batch) -> Self::Shell {
        smallvec![batch]
    }
    fn push(shell: &mut Self::Shell, batch: Self::Batch) {
        shell.push(batch);
    }
    fn batches_mut(shell: &mut Self::Shell) -> &mut [Self::Batch] {
        shell.as_mut()
    }
}
