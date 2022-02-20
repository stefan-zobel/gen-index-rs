#![forbid(unsafe_code)]

mod graph;

use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};

const SLOT_INDEX_BITS: u64 = 36;
const SLOT_INDEX_MASK: u64 = (1 << SLOT_INDEX_BITS) - 1;

const SLOT_GENERATION_BITS: u32 = 28;
const SLOT_GENERATION_MASK: u32 = (1 << SLOT_GENERATION_BITS) - 1;

const CTRL_BLOCK_IDX: usize = 0usize;
const ONE: usize = 1usize;
const DEFAULT_CAPACITY: usize = 16usize;

const ERR_CAP_MSG: &str = "capacity overflow:";
const ERR_CTRL_MSG: &str = "control block is not empty!";

#[derive(Debug)]
pub struct Pool<T> {
    data: Vec<Slot<T>>,
    num_taken: usize,
    max_taken_pos: usize,
}

impl<T> Pool<T> {
    #[inline]
    pub fn new() -> Self {
        Pool::with_capacity(DEFAULT_CAPACITY)
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = {
            if capacity <= DEFAULT_CAPACITY {
                DEFAULT_CAPACITY + ONE
            } else if capacity > SLOT_INDEX_MASK as usize + ONE {
                panic!("{} {}", ERR_CAP_MSG, capacity)
            } else {
                capacity + ONE
            }
        };
        let mut pool = Pool {
            data: Vec::<Slot<T>>::with_capacity(capacity),
            num_taken: 0usize,
            max_taken_pos: 0usize,
        };
        Pool::init_pool(&mut pool);
        pool
    }

    #[inline]
    pub fn add(&mut self, value: T) -> Index {
        self.num_taken += ONE;
        let max_taken = self.max_taken_pos;
        let (control, rest) = self.data.split_at_mut(CTRL_BLOCK_IDX + ONE);
        if let [Slot::Empty { an_empty, .. }] = control {
            let next_free_in_control = an_empty;
            if *next_free_in_control > max_taken {
                self.max_taken_pos = *next_free_in_control;
            }
            let free_slot_index_in_slice = *next_free_in_control - ONE;
            let slot = &mut rest[free_slot_index_in_slice];
            match slot {
                Slot::Empty { an_empty, gen } => {
                    let new_next_empty = *an_empty;
                    let next_gen = *gen;
                    let index = Index::new(*next_free_in_control as u64, next_gen);
                    // this may write usize::MAX into the control block
                    *next_free_in_control = new_next_empty;
                    *slot = Slot::new_taken(value, next_gen);
                    // check 'new_next_empty' that we haven't reached usize::MAX which is invalid
                    if new_next_empty == usize::MAX {
                        self.grow();
                    }
                    return index;
                }
                Slot::Taken { .. } => {
                    panic!("index {} is already taken!", *next_free_in_control);
                }
            }
        }
        panic!("{}", ERR_CTRL_MSG);
    }

    //noinspection ALL
    #[allow(unused)]
    #[inline]
    pub(crate) fn remove_by_pos(&mut self, pos: usize) {
        if pos > 0 && pos < self.data.len() {
            self.num_taken -= ONE;
            let max_taken = self.max_taken_pos;
            let (control, rest) = self.data.split_at_mut(CTRL_BLOCK_IDX + ONE);
            if let [Slot::Empty { an_empty, .. }] = control {
                let next_free_in_control = an_empty;
                let taken_slot_index_in_slice = pos - ONE;
                let slot = &mut rest[taken_slot_index_in_slice];
                match slot {
                    Slot::Taken { gen, .. } => {
                        *slot = Slot::new_empty(*next_free_in_control, *gen + 1u32);
                        *next_free_in_control = pos;
                        if pos == max_taken {
                            for i in (CTRL_BLOCK_IDX..taken_slot_index_in_slice).rev() {
                                if rest[i].is_taken() {
                                    self.max_taken_pos = i + ONE;
                                    break;
                                }
                            }
                            // if all slots to the left were empty
                            // or 'taken_slot_index_in_slice' was 0
                            if self.max_taken_pos == max_taken {
                                self.max_taken_pos = CTRL_BLOCK_IDX;
                            }
                        }
                    }
                    Slot::Empty { .. } => {
                        panic!("index {} is already empty!", pos);
                    }
                }
            } else {
                panic!("{}", ERR_CTRL_MSG);
            }
        }
    }

    //noinspection ALL
    #[inline]
    pub fn remove(&mut self, index: Index) -> Option<T> {
        let pos = index.index() as usize;
        if pos > 0 && pos < self.data.len() {
            let (control, rest) = self.data.split_at_mut(CTRL_BLOCK_IDX + ONE);
            if let [Slot::Empty { an_empty, .. }] = control {
                let next_free_in_control = an_empty;
                let taken_slot_index_in_slice = pos - ONE;
                let slot = &mut rest[taken_slot_index_in_slice];
                if let Slot::Taken { gen, .. } = slot {
                    if *gen == index.generation() {
                        let next_gen = *gen + 1u32;
                        let old = std::mem::replace(
                            slot,
                            Slot::new_empty(*next_free_in_control, next_gen),
                        );
                        *next_free_in_control = pos;
                        self.num_taken -= ONE;
                        let max_taken = self.max_taken_pos;
                        if pos == max_taken {
                            for i in (CTRL_BLOCK_IDX..taken_slot_index_in_slice).rev() {
                                if rest[i].is_taken() {
                                    self.max_taken_pos = i + ONE;
                                    break;
                                }
                            }
                            // if all slots to the left were empty
                            // or 'taken_slot_index_in_slice' was 0
                            if self.max_taken_pos == max_taken {
                                self.max_taken_pos = CTRL_BLOCK_IDX;
                            }
                        }
                        if let Slot::Taken { val, .. } = old {
                            return Some(val);
                        }
                    }
                }
            } else {
                panic!("{}", ERR_CTRL_MSG);
            }
        }
        None
    }

    #[inline]
    pub(crate) fn get_control_block_empty_pointer(&self) -> usize {
        if let Slot::Empty { an_empty, .. } = &self.data[CTRL_BLOCK_IDX] {
            return *an_empty;
        }
        panic!("{}", ERR_CTRL_MSG);
    }

    #[inline]
    pub fn get(&self, index: Index) -> Option<&T> {
        let (pos, version) = index.parts();
        if pos > 0 && pos < self.data.len() {
            if let Slot::Taken { val, gen } = &self.data[pos] {
                if *gen == version {
                    return Some(val);
                }
            }
        }
        None
    }

    #[inline]
    pub fn get_mut(&mut self, index: Index) -> Option<&mut T> {
        let (pos, version) = index.parts();
        if pos > 0 && pos < self.data.len() {
            if let Slot::Taken { val, gen } = self.data.get_mut(pos)? {
                if *gen == version {
                    return Some(val);
                }
            }
        }
        None
    }

    #[inline]
    pub fn size(&self) -> u64 {
        (self.data.len() - ONE) as u64
    }

    #[inline]
    pub fn taken(&self) -> u64 {
        self.num_taken as u64
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.num_taken == 0usize
    }

    #[inline]
    pub fn clear(&mut self) {
        let length = self.data.len();
        // fix the control block
        self.fix_slot_pointer(CTRL_BLOCK_IDX, ONE, true);
        self.num_taken = 0usize;
        self.max_taken_pos = CTRL_BLOCK_IDX;
        // reset the slots
        for i in ONE..length {
            let slot = &mut self.data[i];
            match slot {
                Slot::Empty { an_empty, .. } => {
                    *an_empty = i + ONE;
                }
                Slot::Taken { gen, .. } => {
                    *slot = Slot::new_empty(i + ONE, *gen + 1u32);
                }
            }
        }
        // fix the 'an_empty' pointer in the last slot
        self.fix_slot_pointer(length - ONE, usize::MAX, true);
    }

    #[inline]
    pub fn shrink_to_fit(&mut self) {
        // always keep one extra free Slot
        self.data.truncate(self.max_taken_pos + (ONE + ONE));
        self.data.shrink_to(self.max_taken_pos + (ONE + ONE));
        let length = self.data.len();
        if self.get_control_block_empty_pointer() > length - ONE {
            self.fix_slot_pointer(CTRL_BLOCK_IDX, length - ONE, true);
        }
        // fix the 'an_empty' pointer in the last slot
        self.fix_slot_pointer(length - ONE, usize::MAX, true);
    }

    #[inline]
    fn init_pool(pool: &mut Pool<T>) {
        let capacity = pool.data.capacity();
        for i in 0..capacity {
            pool.data.push(Slot::initial_empty(i + ONE));
        }
        // fix the 'an_empty' pointer in the last slot
        pool.fix_slot_pointer(capacity - ONE, usize::MAX, true);
    }

    #[inline]
    fn grow(&mut self) {
        let old_capacity = self.data.capacity();
        let new_capacity = old_capacity + (old_capacity >> 1);
        if new_capacity > SLOT_INDEX_BITS as usize + ONE {
            panic!("{} {}", ERR_CAP_MSG, new_capacity);
        }
        self.data.reserve_exact(new_capacity - old_capacity);
        let new_capacity = self.data.capacity();
        if new_capacity > old_capacity {
            // fix the control block if necessary
            self.fix_slot_pointer(CTRL_BLOCK_IDX, old_capacity, false);
            // fix the old 'an_empty' pointer if necessary
            self.fix_slot_pointer(old_capacity - ONE, old_capacity, false);
            // initialize the additional slots
            for i in old_capacity..new_capacity {
                self.data.push(Slot::initial_empty(i + ONE));
            }
            // fix the 'an_empty' pointer in the last slot
            self.fix_slot_pointer(new_capacity - ONE, usize::MAX, true);
        }
    }

    #[inline]
    fn fix_slot_pointer(&mut self, position: usize, new_empty: usize, unconditionally: bool) {
        if let Some(Slot::Empty { an_empty, .. }) = self.data.get_mut(position) {
            if unconditionally || *an_empty == usize::MAX {
                *an_empty = new_empty;
            }
        }
    }

    #[inline]
    pub fn iter(&self) -> Iter<T> {
        let mut enumerator = self.data.iter().enumerate();
        // skip control block
        enumerator.next();
        Iter {
            inner: enumerator,
            remaining: self.num_taken,
            upper_bound: self.max_taken_pos,
        }
    }

    #[inline]
    pub fn iter_mut(&mut self) -> IterMut<T> {
        let mut enumerator = self.data.iter_mut().enumerate();
        // skip control block
        enumerator.next();
        IterMut {
            inner: enumerator,
            remaining: self.num_taken,
            upper_bound: self.max_taken_pos,
        }
    }

    #[inline]
    pub fn entries(&self) -> Entry<T> {
        let mut enumerator = self.data.iter().enumerate();
        // skip control block
        enumerator.next();
        Entry {
            inner: enumerator,
            remaining: self.num_taken,
            upper_bound: self.max_taken_pos,
        }
    }

    #[inline]
    pub fn entries_mut(&mut self) -> EntryMut<T> {
        let mut enumerator = self.data.iter_mut().enumerate();
        // skip control block
        enumerator.next();
        EntryMut {
            inner: enumerator,
            remaining: self.num_taken,
            upper_bound: self.max_taken_pos,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Iter<'a, T: 'a> {
    inner: std::iter::Enumerate<std::slice::Iter<'a, Slot<T>>>,
    remaining: usize,
    upper_bound: usize,
}

#[derive(Debug)]
pub struct IterMut<'a, T: 'a> {
    inner: std::iter::Enumerate<std::slice::IterMut<'a, Slot<T>>>,
    remaining: usize,
    upper_bound: usize,
}

#[derive(Clone, Debug)]
pub struct Entry<'a, T: 'a> {
    inner: std::iter::Enumerate<std::slice::Iter<'a, Slot<T>>>,
    remaining: usize,
    upper_bound: usize,
}

#[derive(Debug)]
pub struct EntryMut<'a, T: 'a> {
    inner: std::iter::Enumerate<std::slice::IterMut<'a, Slot<T>>>,
    remaining: usize,
    upper_bound: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next() {
                Some((_, &Slot::Taken { ref val, .. })) => {
                    self.remaining -= ONE;
                    return Some(val);
                }
                Some((pos, &Slot::Empty { .. })) => {
                    if pos > self.upper_bound {
                        assert_eq!(self.remaining, 0usize);
                        break None;
                    } else {
                        continue;
                    }
                }
                None => {
                    assert_eq!(self.remaining, 0usize);
                    return None;
                }
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a, T> Iterator for Entry<'a, T> {
    type Item = (Index, &'a T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next() {
                Some((pos, &Slot::Taken { ref val, gen })) => {
                    self.remaining -= ONE;
                    return Some((Index::new(pos as u64, gen), val));
                }
                Some((pos, &Slot::Empty { .. })) => {
                    if pos > self.upper_bound {
                        assert_eq!(self.remaining, 0usize);
                        break None;
                    } else {
                        continue;
                    }
                }
                None => {
                    assert_eq!(self.remaining, 0usize);
                    return None;
                }
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next() {
                Some((_, &mut Slot::Taken { ref mut val, .. })) => {
                    self.remaining -= ONE;
                    return Some(val);
                }
                Some((pos, &mut Slot::Empty { .. })) => {
                    if pos > self.upper_bound {
                        assert_eq!(self.remaining, 0usize);
                        break None;
                    } else {
                        continue;
                    }
                }
                None => {
                    assert_eq!(self.remaining, 0usize);
                    return None;
                }
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a, T> Iterator for EntryMut<'a, T> {
    type Item = (Index, &'a mut T);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.inner.next() {
                Some((pos, &mut Slot::Taken { ref mut val, gen })) => {
                    self.remaining -= ONE;
                    return Some((Index::new(pos as u64, gen), val));
                }
                Some((pos, &mut Slot::Empty { .. })) => {
                    if pos > self.upper_bound {
                        assert_eq!(self.remaining, 0usize);
                        break None;
                    } else {
                        continue;
                    }
                }
                None => {
                    assert_eq!(self.remaining, 0usize);
                    return None;
                }
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a, T> IntoIterator for &'a Pool<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut Pool<T> {
    type Item = &'a mut T;
    type IntoIter = IterMut<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T> Default for Pool<T> {
    #[inline]
    fn default() -> Self {
        Pool::new()
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Slot<T> {
    Empty { an_empty: usize, gen: u32 },
    Taken { val: T, gen: u32 },
}

impl<T> Slot<T> {
    #[allow(unused)]
    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        matches!(self, Slot::Empty { .. })
    }

    #[inline]
    pub(crate) fn is_taken(&self) -> bool {
        matches!(self, Slot::Taken { .. })
    }

    #[allow(unused)]
    #[inline]
    pub(crate) fn generation(&self) -> u32 {
        match self {
            Slot::Empty { gen, .. } => *gen,
            Slot::Taken { gen, .. } => *gen,
        }
    }

    #[allow(unused)]
    #[inline]
    pub(crate) fn get(&self) -> Option<&T> {
        match self {
            Slot::Empty { .. } => None,
            Slot::Taken { val, .. } => Some(val),
        }
    }

    #[allow(unused)]
    #[inline]
    pub(crate) fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Slot::Empty { .. } => None,
            Slot::Taken { val, .. } => Some(val),
        }
    }

    #[inline]
    pub(crate) fn new_taken(val: T, gen: u32) -> Self {
        Slot::Taken { val, gen }
    }

    #[inline]
    pub(crate) fn new_empty(next_free: usize, gen: u32) -> Self {
        Slot::Empty {
            an_empty: next_free,
            gen,
        }
    }

    #[inline]
    pub(crate) fn initial_empty(next_free: usize) -> Self {
        Slot::new_empty(next_free, 0u32)
    }
}

/// An `Index` reserves 36 bits for the position, 28 bits for the generation.
#[derive(Clone, Copy, Debug, Eq)]
pub struct Index {
    id: u64,
}

impl Ord for Index {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        match self.index().cmp(&other.index()) {
            Ordering::Less => Ordering::Less,
            Ordering::Equal => self.id.cmp(&other.id),
            Ordering::Greater => Ordering::Greater,
        }
    }
}

impl PartialOrd for Index {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Index {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Hash for Index {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Index {
    #[inline]
    pub fn index(&self) -> u64 {
        self.id & SLOT_INDEX_MASK
    }

    #[inline]
    pub fn generation(&self) -> u32 {
        (self.id >> SLOT_INDEX_BITS) as u32 & SLOT_GENERATION_MASK
    }

    #[inline]
    pub(crate) const fn new(idx: u64, gen: u32) -> Self {
        let id = (idx & SLOT_INDEX_MASK)
            | (((gen & SLOT_GENERATION_MASK) as u64) << (64 - SLOT_GENERATION_BITS));
        Index { id }
    }

    #[inline]
    pub fn parts(&self) -> (usize, u32) {
        (self.index() as usize, self.generation())
    }
}

impl Display for Index {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "idx: {}, gen: {}", self.index(), self.generation())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn print_sizes() {
        let size1 = size_of::<Slot<()>>();
        println!("size: {}", size1);
        println!("SLOT_INDEX_MASK: {}", format!("{:#16x}", SLOT_INDEX_MASK));
        println!(
            "SLOT_GENERATION_MASK: {}",
            format!("{:#8x}", SLOT_GENERATION_MASK)
        );
    }

    #[test]
    fn print_index() {
        let idx = Index::new(1, 268_435_455);
        println!("Index Display: {}", idx);
        println!("Index Debug  : {:?}", idx);
    }

    #[test]
    fn test_create_pool_then_add_and_remove_by_pos() {
        let mut pool = Pool::<i32>::new();
        println!("pool: {:?}\n", pool);
        // add 42, gen=0 at 1
        let elem = 42i32;
        let index = pool.add(elem);
        println!("new Index: {}", index);
        assert_eq!(1, index.index());
        println!("pool: {:?}\n", pool);
        // add 49, gen=0 at 2
        let elem = 49i32;
        let index = pool.add(elem);
        println!("new Index: {}", index);
        assert_eq!(2, index.index());
        println!("pool: {:?}\n", pool);
        pool.remove_by_pos(0);
        pool.remove_by_pos(17);
        // remove 42, gen=0 from 1
        pool.remove_by_pos(1);
        println!("pool: {:?}\n", pool);
        // add 51, gen=1 at 1
        let elem = 51i32;
        let index = pool.add(elem);
        println!("pool: {:?}\n", pool);
        assert_eq!(1, index.index());
        println!("new Index: {}", index);
        // remove 49, gen=0 from 2
        pool.remove_by_pos(2);
        println!("pool: {:?}\n", pool);
        // add 53, gen=1 at 2
        let elem = 53i32;
        let index = pool.add(elem);
        assert_eq!(2, index.index());
        println!("new Index: {}", index);
        println!("pool: {:?}\n", pool);
        // remove 51, gen=1 from 1
        pool.remove_by_pos(1);
        println!("pool: {:?}\n", pool);
        // remove 53, gen=1 from 2
        pool.remove_by_pos(2);
        println!("pool: {:?}\n", pool);
        // add 66, gen=2 at 2
        let elem = 66i32;
        let index = pool.add(elem);
        assert_eq!(2, index.index());
        println!("new Index: {}", index);
        println!("pool: {:?}\n", pool);
        // add 77, gen=2 at 1
        let elem = 77i32;
        let index = pool.add(elem);
        assert_eq!(1, index.index());
        println!("new Index: {}", index);
        println!("pool: {:?}\n", pool);
        pool.grow();
        println!("pool: {:?}\n", pool);
        pool.clear();
        println!("Pool after clear():\n {:?}\n", pool);
    }

    #[test]
    fn test_resize() {
        let mut pool = Pool::<i32>::new();
        // add entries from 1 to 15, last slot remains empty
        for i in 1..16 {
            pool.add(i);
        }
        println!("Pool after first insert round:\n {:?}\n", pool);
        println!("pool size: {}\n", pool.size());
        // use up the last slot (store 16 in index 16)
        pool.add(16);
        println!("Pool after additional insert:\n {:?}\n", pool);
        println!("pool size: {}\n", pool.size());
        // add 8 more from 17 to 24
        for i in 17..25 {
            pool.add(i);
        }
        println!("Pool after second insert round:\n {:?}\n", pool);
        println!("pool size: {}\n", pool.size());
        assert_eq!(37, pool.data.capacity());
        assert_eq!(37, pool.data.len());
        assert_eq!(36, pool.size());
        // add 25 to slot 25
        pool.add(25);
        println!("Pool after additional insert:\n {:?}\n", pool);
        println!("pool size: {}\n", pool.size());
    }

    #[test]
    fn test_remove() {
        let mut pool = Pool::<i32>::new();
        let idx = pool.add(15);
        let val = pool.remove(idx);
        println!("got back : {:?}", val);
        println!("Pool after removal:\n {:?}\n", pool);
    }

    #[test]
    fn test_max_taken_remove_by_pos() {
        println!("MAX_TAKEN_REMOVE_TEST");
        let mut pool = Pool::<i32>::new();
        println!("max_taken_pos after init: {}", pool.max_taken_pos);
        assert_eq!(0, pool.max_taken_pos);
        let _idx = pool.add(1);
        println!("max_taken_pos: {}", pool.max_taken_pos);
        assert_eq!(1, pool.max_taken_pos);
        pool.remove_by_pos(1);
        println!("max_taken_pos after remove: {}", pool.max_taken_pos);
        assert_eq!(0, pool.max_taken_pos);
        let _idx = pool.add(1);
        assert_eq!(1, pool.max_taken_pos);
        let _idx = pool.add(2);
        assert_eq!(2, pool.max_taken_pos);
        let _idx = pool.add(3);
        assert_eq!(3, pool.max_taken_pos);
        pool.remove_by_pos(2);
        assert_eq!(3, pool.max_taken_pos);
        pool.remove_by_pos(3);
        assert_eq!(1, pool.max_taken_pos);
        pool.remove_by_pos(1);
        assert_eq!(0, pool.max_taken_pos);
        println!("Pool:\n {:?}\n", pool);
        let _idx = pool.add(1);
        assert_eq!(1, pool.max_taken_pos);
        println!("Pool:\n {:?}\n", pool);
        let _idx = pool.add(2);
        assert_eq!(3, pool.max_taken_pos);
        println!("Pool:\n {:?}\n", pool);
        let _idx = pool.add(3);
        assert_eq!(3, pool.max_taken_pos);
        println!("Pool:\n {:?}\n", pool);
        let _idx = pool.add(4);
        assert_eq!(4, pool.max_taken_pos);
        println!("Pool:\n {:?}\n", pool);
        pool.remove_by_pos(4);
        println!("Pool:\n {:?}\n", pool);
        assert_eq!(3, pool.max_taken_pos);
        pool.remove_by_pos(3);
        assert_eq!(2, pool.max_taken_pos);
        pool.remove_by_pos(2);
        assert_eq!(1, pool.max_taken_pos);
        println!("Pool:\n {:?}\n", pool);
        pool.remove_by_pos(1);
        assert_eq!(0, pool.max_taken_pos);
        println!("Pool:\n {:?}\n", pool);
    }

    #[test]
    fn test_iter() {
        println!("test_iter()");
        let mut pool = Pool::<i32>::with_capacity(1024);
        // add 16 items in order
        for i in 1..17 {
            pool.add(i);
        }
        // remove the first 14 items
        for i in 1..15 {
            pool.remove_by_pos(i);
        }
        // iterate over the remaining 2 items
        let mut i: u32 = 0;
        for elem in pool.iter() {
            if i == 0 {
                assert_eq!(15, *elem);
            } else {
                assert_eq!(16, *elem);
            }
            println!("iter element: {}", *elem);
            i += 1;
        }
    }

    #[test]
    fn test_iter_mut() {
        println!("test_iter_mut()");
        let mut pool = Pool::<i32>::with_capacity(1024);
        // add 16 items in order
        for i in 1..17 {
            pool.add(i);
        }
        // remove the first 14 items
        for i in 1..15 {
            pool.remove_by_pos(i);
        }
        // iterate over the remaining 2 items
        let mut i: u32 = 0;
        for elem in pool.iter_mut() {
            if i == 0 {
                assert_eq!(15, *elem);
            } else {
                assert_eq!(16, *elem);
            }
            println!("iter element: {}", *elem);
            // multiply by 2
            *elem = 2 * *elem;
            i += 1;
        }
        i = 0;
        for changed in pool.iter() {
            if i == 0 {
                assert_eq!(30, *changed);
            } else {
                assert_eq!(32, *changed);
            }
            println!("iter element: {}", *changed);
            i += 1;
        }
    }

    #[test]
    fn test_into_iter() {
        println!("test_into_iter()");
        let mut pool = Pool::<i32>::with_capacity(1024);
        // add 16 items in order
        for i in 1..17 {
            pool.add(i);
        }
        // remove the first 14 items
        for i in 1..15 {
            pool.remove_by_pos(i);
        }
        // iterate over the remaining 2 items
        let mut i: u32 = 0;
        for elem in &pool {
            if i == 0 {
                assert_eq!(15, *elem);
            } else {
                assert_eq!(16, *elem);
            }
            println!("iter element: {}", *elem);
            i += 1;
        }
    }

    #[test]
    fn test_into_iter_mut() {
        println!("test_into_iter_mut()");
        let mut pool = Pool::<i32>::with_capacity(1024);
        // add 16 items in order
        for i in 1..17 {
            pool.add(i);
        }
        // remove the first 14 items
        for i in 1..15 {
            pool.remove_by_pos(i);
        }
        // iterate over the remaining 2 items
        let mut i: u32 = 0;
        for elem in &mut pool {
            if i == 0 {
                assert_eq!(15, *elem);
            } else {
                assert_eq!(16, *elem);
            }
            println!("iter element: {}", *elem);
            // multiply by 2
            *elem = 2 * *elem;
            i += 1;
        }
        i = 0;
        for changed in &pool {
            if i == 0 {
                assert_eq!(30, *changed);
            } else {
                assert_eq!(32, *changed);
            }
            println!("iter element: {}", *changed);
            i += 1;
        }
    }

    #[test]
    fn test_shrink_to_fit() {
        println!("test_shrink_to_fit:");
        let mut pool = Pool::<i32>::with_capacity(1024);
        // add 16 items in order
        for i in 1..17 {
            pool.add(i);
        }
        // remove the first 14 items
        for i in 1..15 {
            pool.remove_by_pos(i);
        }
        pool.shrink_to_fit();
        println!("A) Pool after shrinking:\n {:?}\n", pool);
        assert_eq!(2, pool.taken());
        assert_eq!(16, pool.max_taken_pos);
        assert_eq!(14, pool.get_control_block_empty_pointer());
        let mut pool = Pool::<i32>::with_capacity(1024);
        // add 16 items in order
        for i in 1..17 {
            pool.add(i);
        }
        pool.shrink_to_fit();
        println!("B) Pool after shrinking:\n {:?}\n", pool);
        assert_eq!(16, pool.taken());
        assert_eq!(16, pool.max_taken_pos);
        assert_eq!(17, pool.get_control_block_empty_pointer());
    }

    #[test]
    fn test_shrink_to_fit_when_empty() {
        println!("test_shrink_to_fit_when_empty:");
        let mut pool = Pool::<i32>::with_capacity(10);
        println!("Pool before shrinking:\n {:?}\n", pool);
        pool.shrink_to_fit();
        println!("Pool after shrinking:\n {:?}\n", pool);
        assert_eq!(0, pool.taken());
        assert_eq!(0, pool.max_taken_pos);
        assert_eq!(1, pool.get_control_block_empty_pointer());
    }

    //noinspection ALL
    #[test]
    fn test_entries() {
        println!("test_entries()");
        let mut pool = Pool::<i32>::with_capacity(1024);
        // add 16 items in order
        for i in 1..17 {
            pool.add(i);
        }
        // remove the first 14 items
        for i in 1..15 {
            pool.remove_by_pos(i);
        }
        // iterate over the remaining 2 items
        let mut i: u32 = 0;
        for elem in pool.entries() {
            if i == 0 {
                assert_eq!(15, elem.0.index());
                assert_eq!(15, *elem.1);
            } else {
                assert_eq!(16, elem.0.index());
                assert_eq!(16, *elem.1);
            }
            println!("iter element: {:?}", elem);
            i += 1;
        }
    }

    //noinspection ALL
    #[test]
    fn test_entries_mut() {
        println!("test_entries_mut()");
        let mut pool = Pool::<i32>::with_capacity(1024);
        // add 16 items in order
        for i in 1..17 {
            pool.add(i);
        }
        // remove the first 14 items
        for i in 1..15 {
            pool.remove_by_pos(i);
        }
        // iterate over the remaining 2 items
        let mut i: u32 = 0;
        for elem in pool.entries_mut() {
            if i == 0 {
                assert_eq!(15, elem.0.index());
                assert_eq!(15, *elem.1);
            } else {
                assert_eq!(16, elem.0.index());
                assert_eq!(16, *elem.1);
            }
            println!("iter element: {:?}", elem);
            // multiply by 2
            *elem.1 = 2 * *elem.1;
            i += 1;
        }
        i = 0;
        for changed in pool.entries() {
            if i == 0 {
                assert_eq!(15, changed.0.index());
                assert_eq!(30, *changed.1);
            } else {
                assert_eq!(16, changed.0.index());
                assert_eq!(32, *changed.1);
            }
            println!("iter element: {:?}", changed);
            i += 1;
        }
    }
}
