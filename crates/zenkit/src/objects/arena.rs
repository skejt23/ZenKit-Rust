//! Dense, fallibly allocated pages. Public object identities never use addresses.
use super::{ErrorKind, Result, bad};
use std::mem::size_of;

const PAGE_BYTES: usize = 16 * 1024;

#[cfg(test)]
std::thread_local! {
    static FAIL_AFTER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}
fn allocation() -> Result<()> {
    #[cfg(test)]
    FAIL_AFTER.with(|remaining| match remaining.get() {
        Some(0) => Err(bad(
            ErrorKind::LimitExceeded,
            "injected arena allocation failure",
        )),
        Some(n) => {
            remaining.set(Some(n - 1));
            Ok(())
        }
        None => Ok(()),
    })?;
    Ok(())
}
#[cfg(test)]
pub(crate) fn with_allocation_failure<T>(after: usize, f: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            FAIL_AFTER.with(|v| v.set(None));
        }
    }
    FAIL_AFTER.with(|v| {
        assert!(v.get().is_none());
        v.set(Some(after));
    });
    let _reset = Reset;
    f()
}

pub(crate) struct Arena<T> {
    pages: Vec<Vec<T>>,
    len: usize,
}

/// New pages stay outside the arena until the first live element is committed.
pub(crate) struct Prepared<T>(Option<Vec<T>>);

pub(crate) struct Stored<T> {
    pub(crate) owner: usize,
    pub(crate) value: T,
}

/// Internal replacement dispatch; `Different` is an incoming value, not an error.
pub(crate) enum Replacement<T, L = ()> {
    Replaced(T, L),
    Different(T),
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self {
            pages: Vec::new(),
            len: 0,
        }
    }
}

impl<T> Arena<T> {
    fn page_len() -> usize {
        let count = (PAGE_BYTES / size_of::<T>().max(1)).max(1);
        1usize << count.ilog2()
    }
    pub(crate) fn len(&self) -> usize {
        self.len
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub(crate) fn prepare(&mut self) -> Result<Prepared<T>> {
        self.len
            .checked_add(1)
            .ok_or_else(|| bad(ErrorKind::LimitExceeded, "arena length"))?;
        let page_len = Self::page_len();
        if let Some(page) = self.pages.last_mut()
            && page.len() < page_len
        {
            if page.len() == page.capacity() {
                let capacity = page.capacity().max(2).saturating_mul(2).min(page_len);
                allocation()?;
                page.try_reserve_exact(capacity - page.len())
                    .map_err(|_| bad(ErrorKind::LimitExceeded, "arena first page allocation"))?;
            }
            return Ok(Prepared(None));
        }
        if self.pages.len() == self.pages.capacity() {
            allocation()?;
        }
        self.pages
            .try_reserve(1)
            .map_err(|_| bad(ErrorKind::LimitExceeded, "arena page directory allocation"))?;
        let capacity = if self.pages.is_empty() {
            page_len.min(4)
        } else {
            page_len
        };
        let mut page = Vec::new();
        allocation()?;
        page.try_reserve_exact(capacity)
            .map_err(|_| bad(ErrorKind::LimitExceeded, "arena page allocation"))?;
        Ok(Prepared(Some(page)))
    }
    pub(crate) fn commit(&mut self, prepared: Prepared<T>, value: T) -> usize {
        let index = self.len;
        if let Some(mut page) = prepared.0 {
            assert!(
                self.pages
                    .last()
                    .is_none_or(|p| p.len() == Self::page_len())
            );
            page.push(value);
            self.pages.push(page);
        } else {
            let page = self.pages.last_mut().expect("prepared existing page");
            assert!(page.len() < page.capacity() && page.len() < Self::page_len());
            page.push(value);
        }
        self.len += 1;
        index
    }
    pub(crate) fn get(&self, index: usize) -> &T {
        assert!(index < self.len);
        &self.pages[index / Self::page_len()][index % Self::page_len()]
    }
    pub(crate) fn get_mut(&mut self, index: usize) -> &mut T {
        assert!(index < self.len);
        &mut self.pages[index / Self::page_len()][index % Self::page_len()]
    }
    pub(crate) fn swap_remove(&mut self, index: usize) -> T {
        assert!(index < self.len);
        let last_page = self.pages.last_mut().expect("nonempty arena");
        let last = last_page.pop().expect("no published empty page");
        if last_page.is_empty() {
            self.pages.pop();
        }
        self.len -= 1;
        if index == self.len {
            last
        } else {
            std::mem::replace(self.get_mut(index), last)
        }
    }
    pub(crate) fn iter(&self) -> Iter<'_, T> {
        Iter {
            arena: self,
            front: 0,
            back: self.len,
        }
    }
}

pub(crate) struct Iter<'a, T> {
    arena: &'a Arena<T>,
    front: usize,
    back: usize,
}
impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        let index = self.front;
        self.front += 1;
        Some(self.arena.get(index))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let count = self.back - self.front;
        (count, Some(count))
    }
}
impl<T> DoubleEndedIterator for Iter<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        self.back -= 1;
        Some(self.arena.get(self.back))
    }
}
impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<T> std::iter::FusedIterator for Iter<'_, T> {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_remain_dense_across_preparation_and_removal() {
        let mut arena = Arena::<usize>::default();
        let count = Arena::<usize>::page_len() * 2 + 1;
        for i in 0..count {
            let p = arena.prepare().unwrap();
            arena.commit(p, i);
        }
        assert_eq!(
            arena.iter().copied().collect::<Vec<_>>(),
            (0..count).collect::<Vec<_>>()
        );
        assert_eq!(arena.swap_remove(3), 3);
        assert_eq!(*arena.get(3), count - 1);
        assert_eq!(arena.pages.len(), 2);
        let pending = arena.prepare().unwrap();
        assert_eq!(arena.pages.len(), 2); // pending third page is not published
        drop(pending);
        assert_eq!(arena.len(), count - 1);
        let p = arena.prepare().unwrap();
        arena.commit(p, 999);
        assert_eq!(*arena.get(count - 1), 999);
        while !arena.is_empty() {
            arena.swap_remove(arena.len() - 1);
        }
        assert!(arena.pages.is_empty());
    }
}
