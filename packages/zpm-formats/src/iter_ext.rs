use crate::{Entry, Error};

pub trait IterExt {
    fn strip_first_segment(self) -> StripFirstSegment<Self> where Self: Sized;
}

impl<'a, T> IterExt for T where T: Iterator<Item = Result<Entry<'a>, Error>> {
    fn strip_first_segment(self) -> StripFirstSegment<Self> {
        StripFirstSegment { iter: self }
    }
}

pub struct StripFirstSegment<T> {
    pub(crate) iter: T,
}

impl<'a, T> Iterator for StripFirstSegment<T> where T: Iterator<Item = Result<Entry<'a>, Error>> {
    type Item = Result<Entry<'a>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let next
                = self.iter.next();

            let mut next = match next {
                None => return None,
                Some(Err(err)) => return Some(Err(err)),
                Some(Ok(entry)) => entry,
            };

            if let Some(slash_index) = next.name.find('/') {
                next.name = next.name[slash_index + 1..].to_string();
                return Some(Ok(next));
            }
        }
    }
}
