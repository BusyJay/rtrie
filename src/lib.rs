use std::mem;
use std::borrow::Cow;


enum PosType {
    Leaf,
    Edge(usize),
    Child(usize),
}

struct SearchKey<'a> {
    base: Cow<'a, [u8]>,
    offset: usize,
}

impl<'a> AsRef<[u8]> for SearchKey<'a> {
    fn as_ref(&self) -> &[u8] {
        &self.base[self.offset..]
    }
}

impl<'a> SearchKey<'a> {
    pub fn new(key: Vec<u8>) -> SearchKey<'a> {
        SearchKey {
            base: Cow::Owned(key),
            offset: 0,
        }
    }

    pub fn borrow(key: &'a [u8]) -> SearchKey<'a> {
        SearchKey {
            base: Cow::Borrowed(key),
            offset: 0,
        }
    }

    pub fn consume(&mut self, size: usize) {
        self.offset += size;
    }

    #[inline]
    pub fn first(&self) -> u8 {
        self.base[self.offset]
    }

    pub fn is_empty(&self) -> bool {
        self.base.len() == self.offset
    }
}

unsafe fn remove_entry<C>(node: &mut TrieNode<C>, levels: Vec<(*mut TrieNode<C>, usize)>) -> C {
    let data = node.data.take().unwrap();
    if levels.is_empty() || !node.children.is_empty() {
        // Using node can't be deleted.
        return data;
    }

    for (l, pos) in levels {
        let node = &mut *l;
        node.children.remove(pos);
        // TODO: make the check before actually push to self.levels
        if !node.children.is_empty() || node.data.is_some() {
            break;
        }
    }
    data
}

pub struct NotFound<'a, C> {
    node: *mut TrieNode<C>,
    pos: PosType,
    left: SearchKey<'a>,
}

impl<'a, C> NotFound<'a, C> {
    unsafe fn prefix_len(&self) -> usize {
        match self.pos {
            PosType::Child(_) => 0,
            PosType::Edge(_) => {
                if self.left.is_empty() {
                    let node = &*self.node;
                    node.len()
                } else {
                    0
                }
            }
            PosType::Leaf => {
                let node = &* self.node;
                node.len()
            }
        }
    }
}

pub struct Found<'a, C> {
    node: *mut TrieNode<C>,
    key: SearchKey<'a>,
    levels: Vec<(*mut TrieNode<C>, usize)>,
}

impl<'a, C> Found<'a, C> {
    unsafe fn len(&self) -> usize {
        let node = &*self.node;
        node.len()
    }

    unsafe fn remove(self) -> (Vec<u8>, C) {
        let node = &mut *self.node;
        let c = remove_entry(node, self.levels);
        (self.key.base.into_owned(), c)
    }
}

// TODO: don't use recursion.
#[inline]
unsafe fn search_node<'a, C>(node: *mut TrieNode<C>, mut key: SearchKey<'a>, levels: usize) -> Result<Found<C>, NotFound<'a, C>> {
    let n = &mut *node;
    let prefix_size = common_prefix(&n.segment, key.as_ref());
    key.consume(prefix_size);
    if prefix_size != n.segment.len() {
        return Err(NotFound {
            node: node,
            pos: PosType::Edge(prefix_size),
            left: key,
        });
    }
    if !key.is_empty() {
        return match n.children.binary_search_by(|k| k.segment[0].cmp(&key.first())) {
            Ok(i) => {
                let mut e = search_node(&n.children[i] as *const _ as *mut _, key, levels + 1);
                if let Ok(ref mut e) = e {
                    e.levels.push((node, i));
                }
                e
            },
            Err(i) => Err(NotFound {
                node: node,
                pos: PosType::Child(i),
                left: key,
            })
        };
    }
    if n.data.is_none() {
        Err(NotFound {
            node: node,
            pos: PosType::Leaf,
            left: SearchKey::borrow(&[]),
        })
    } else {
        Ok(Found {
            node: node,
            key: key,
            levels: Vec::with_capacity(levels),
        })
    }
}

pub struct VacantEntry<'a, C: 'a> {
    node: &'a mut TrieNode<C>,
    pos: PosType,
    key: Vec<u8>,
    key_off: usize,
}

impl<'a, C> VacantEntry<'a, C> {
    pub fn insert(self, val: C) -> &'a mut C {
        let pos = match self.pos {
            PosType::Edge(pos) => {
                let mut split_child = TrieNode::new();
                mem::swap(&mut split_child.children, &mut self.node.children);
                split_child.segment = self.node.segment[pos..].to_vec();
                mem::swap(&mut split_child.data, &mut self.node.data);
                self.node.segment.truncate(pos);
                self.node.segment.shrink_to_fit();
                self.node.children.push(split_child);
                if self.key.len() == self.key_off {
                    self.node.data = Some(val);
                    return self.node.data.as_mut().unwrap();
                }
                if self.node.children[0].segment[0] <= self.key[self.key_off] {
                    1
                } else {
                    0
                }
            }
            PosType::Child(pos) => pos,
            PosType::Leaf => {
                self.node.data = Some(val);
                return self.node.data.as_mut().unwrap();
            }
        };

        let child = TrieNode {
            segment: if self.key_off == 0 {
                self.key
            } else {
                self.key[self.key_off..].to_vec()
            },
            children: vec![],
            data: Some(val),
        };
        self.node.children.insert(pos, child);
        self.node.children[pos].data.as_mut().unwrap()
    }

    pub fn prefix_len(&self) -> usize {
        match self.pos {
            PosType::Child(_) => 0,
            PosType::Edge(_) => {
                if self.key.len() == self.key_off {
                    self.node.len()
                } else {
                    0
                }
            }
            PosType::Leaf => self.node.len(),
        }
    }

    pub fn key(&self) -> &[u8] {
        &self.key
    }

    pub fn into_key(self) -> Vec<u8> {
        self.key
    }
}

pub struct OccupiedEntry<'a, C: 'a> {
    node: &'a mut TrieNode<C>,
    key: Vec<u8>,
    levels: Vec<(*mut TrieNode<C>, usize)>,
}

impl<'a, C> OccupiedEntry<'a, C> {
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    pub fn remove_entry(self) -> (Vec<u8>, C) {
        let c = unsafe { remove_entry(self.node, self.levels) };
        (self.key,  c)
    }

    pub fn get(&self) -> &C {
        self.node.data.as_ref().unwrap()
    }

    pub fn get_mut(&mut self) -> &mut C {
        self.node.data.as_mut().unwrap()
    }

    pub fn into_mut(self) -> &'a mut C {
        self.node.data.as_mut().unwrap()
    }

    pub fn insert(&mut self, val: C) -> C {
        let previous = self.node.data.take().unwrap();
        self.node.data = Some(val);
        previous
    }

    pub fn remove(self) -> C {
        self.remove_entry().1
    }
}

pub enum Entry<'a, C: 'a> {
    Vacant(VacantEntry<'a, C>),
    Occupied(OccupiedEntry<'a, C>),
}

impl<'a, C: 'a> Entry<'a, C> {
    pub fn or_insert(self, val: C) -> &'a mut C {
        match self {
            Entry::Vacant(v) => v.insert(val),
            Entry::Occupied(o) => o.into_mut(),
        }
    }

    pub fn or_insert_with<F: FnOnce() -> C>(self, f: F) -> &'a mut C {
        match self {
            Entry::Vacant(v) => v.insert(f()),
            Entry::Occupied(o) => o.into_mut(),
        }
    }

    pub fn key(&self) -> &[u8] {
        match *self {
            Entry::Vacant(ref v) => v.key(),
            Entry::Occupied(ref o) => o.key(),
        }
    }
}

pub struct TrieNode<C> {
    segment: Vec<u8>,
    children: Vec<TrieNode<C>>,
    data: Option<C>,
}

fn common_prefix(lhs: &[u8], rhs: &[u8]) -> usize {
    lhs.into_iter().zip(rhs).take_while(|&(l, r)| l == r).count()
}

impl<C> TrieNode<C> {
    pub fn new() -> TrieNode<C> {
        TrieNode {
            segment: Vec::new(),
            children: Vec::new(),
            data: None,
        }
    }

    pub fn entry(&mut self, key: Vec<u8>) -> Entry<C> {
        unsafe {
            match search_node(self, SearchKey::new(key), 0) {
                Ok(f) => {
                    Entry::Occupied(OccupiedEntry {
                        node: &mut *f.node,
                        key: f.key.base.into_owned(),
                        levels: f.levels,
                    })
                }
                Err(f) => {
                    Entry::Vacant(VacantEntry {
                        node: &mut *f.node,
                        key: f.left.base.into_owned(),
                        pos: f.pos,
                        key_off: f.left.offset,
                    })
                }
            }
        }
    }

    pub fn insert(&mut self, key: Vec<u8>, value: C) -> Option<C> {
        match self.entry(key) {
            Entry::Occupied(mut o) => Some(o.insert(value)),
            Entry::Vacant(v) => {
                v.insert(value);
                None
            }
        }
    }

    // TODO: cache len
    pub fn len(&self) -> usize {
        self.children.iter().fold(0, |sum, n| sum + n.len()) + self.data.as_ref().map_or(0, |_| 1)
    }

    pub fn prefix_len(&self, prefix_key: &[u8]) -> usize {
        unsafe {
            match search_node(self as *const _ as *mut TrieNode<C>, SearchKey::borrow(prefix_key), 0) {
                Ok(f) => f.len(),
                Err(f) => f.prefix_len(),
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_none() && self.children.is_empty()
    }

    pub fn get(&self, key: &[u8]) -> Option<&C> {
        unsafe {
            match search_node(self as *const _ as *mut _, SearchKey::borrow(key), 0) {
                Ok(f) => {
                    let node = &mut *f.node;
                    node.data.as_ref()
                },
                Err(_) => None,
            }
        }
    }

    pub fn get_mut(&mut self, key: &[u8]) -> Option<&mut C> {
        unsafe {
            match search_node(self, SearchKey::borrow(key), 0) {
                Ok(f) => {
                    let node = &mut *f.node;
                    node.data.as_mut()
                },
                Err(_) => None,
            }
        }
    }

    pub fn remove(&mut self, key: &[u8]) -> Option<C> {
        unsafe {
            match search_node(self, SearchKey::borrow(key), 0) {
                Ok(f) => Some(f.remove().1),
                Err(_) => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_ops() {
        let cases = vec![
            (vec![1, 2, 3], 2, None, 1),
            (vec![1, 2], 1, None, 2),
            (vec![1, 2, 3], 3, Some(2), 2),
            (vec![1, 2, 3, 5], 4, None, 3),
            (vec![1, 2, 5, 3], 3, None, 4),
        ];

        let mut trie = TrieNode::new();
        assert_eq!(0, trie.len());
        assert!(trie.is_empty());
        for (key, value, returned, len) in cases {
            assert_eq!(returned.as_ref(), trie.get(&key));
            assert_eq!(returned, trie.insert(key.clone(), value.clone()));
            assert_eq!(len, trie.len());
            assert_eq!(Some(&value), trie.get(&key));
        }

        let cases = vec![
            (vec![1, 2, 3], Some(3), 3),
            (vec![1, 2], Some(1), 2),
            (vec![1, 2, 3], None, 2),
            (vec![1, 2, 3, 5], Some(4), 1),
            (vec![1, 2, 5, 3], Some(3), 0),
        ];

        assert_eq!(4, trie.len());
        for (key, returned, len) in cases {
            assert_eq!(returned, trie.remove(&key));
            assert_eq!(len, trie.len());
        }
    }
}
