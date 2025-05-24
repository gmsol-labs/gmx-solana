/// A `slice::chunk_by` implementation, copied from `std`.
#[inline]
pub fn chunk_by<T, F>(slice: &[T], pred: F) -> ChunkBy<'_, T, F>
where
    F: FnMut(&T, &T) -> bool,
{
    ChunkBy::new(slice, pred)
}

/// Iterator returns by [`chunk_by`].
pub struct ChunkBy<'a, T: 'a, P> {
    slice: &'a [T],
    predicate: P,
}

impl<'a, T: 'a, P> ChunkBy<'a, T, P> {
    pub(super) fn new(slice: &'a [T], predicate: P) -> Self {
        ChunkBy { slice, predicate }
    }
}

impl<'a, T: 'a, P> Iterator for ChunkBy<'a, T, P>
where
    P: FnMut(&T, &T) -> bool,
{
    type Item = &'a [T];

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.slice.is_empty() {
            None
        } else {
            let mut len = 1;
            let mut iter = self.slice.windows(2);
            while let Some([l, r]) = iter.next() {
                if (self.predicate)(l, r) {
                    len += 1
                } else {
                    break;
                }
            }
            let (head, tail) = self.slice.split_at(len);
            self.slice = tail;
            Some(head)
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.slice.is_empty() {
            (0, Some(0))
        } else {
            (1, Some(self.slice.len()))
        }
    }
}
