//! Flat, index-based DOM. Nodes are 5 links + a borrowed kind; everything borrows from the source text.
use super::events::{Event, Events};
use super::tokenizer::{Token, Tokenizer};

pub const NIL: u32 = u32::MAX;

#[derive(Debug, Clone, Copy)]
pub enum Kind<'a> {
    Document,
    Element { name: &'a str, attrs: &'a str },
    Text { text: &'a str, decode: bool },
}

#[derive(Debug, Clone, Copy)]
pub struct Node<'a> {
    pub kind: Kind<'a>,
    pub parent: u32,
    pub first_child: u32,
    pub last_child: u32,
    pub next: u32,
    pub prev: u32,
}

pub struct Doc<'a> {
    pub nodes: Vec<Node<'a>>,
}

impl<'a> Doc<'a> {
    /// Node 0 is the virtual document node.
    pub fn parse(src: &'a str) -> Self {
        let mut doc = Doc { nodes: Vec::with_capacity(src.len() / 24 + 8) };
        doc.nodes.push(Node { kind: Kind::Document, parent: NIL, first_child: NIL, last_child: NIL, next: NIL, prev: NIL });
        let mut cur = 0u32;
        for ev in Events::new(src) {
            match ev {
                Event::Open { name, attrs } => cur = doc.append(cur, Kind::Element { name, attrs }),
                Event::Close { .. } => {
                    let p = doc.nodes[cur as usize].parent;
                    if p != NIL {
                        cur = p;
                    }
                }
                Event::Text { text, decode } => {
                    doc.append(cur, Kind::Text { text, decode });
                }
            }
        }
        doc
    }

    /// Parse XML without HTML void-element or raw-text `<title>`/`<link>` quirks; honors `<tag/>` and CDATA.
    pub fn parse_xml(src: &'a str) -> Self {
        let mut doc = Doc { nodes: Vec::with_capacity(src.len() / 24 + 8) };
        doc.nodes.push(Node { kind: Kind::Document, parent: NIL, first_child: NIL, last_child: NIL, next: NIL, prev: NIL });
        let mut cur = 0u32;
        let mut tok = Tokenizer::new_xml(src);
        while let Some(ev) = tok.next_token() {
            match ev {
                Token::Start { name, attrs, self_closing } => {
                    let id = doc.append(cur, Kind::Element { name, attrs });
                    if !self_closing {
                        cur = id;
                    }
                }
                Token::End { name } => {
                    let mut anc = cur;
                    while anc != 0 && anc != NIL {
                        if let Kind::Element { name: open_name, .. } = doc.node(anc).kind {
                            if open_name == name || open_name.eq_ignore_ascii_case(name) {
                                cur = doc.node(anc).parent;
                                break;
                            }
                        }
                        anc = doc.node(anc).parent;
                    }
                }
                Token::Text { text, decode } => {
                    doc.append(cur, Kind::Text { text, decode });
                }
            }
        }
        doc
    }

    fn append(&mut self, parent: u32, kind: Kind<'a>) -> u32 {
        let id = self.nodes.len() as u32;
        let last = self.nodes[parent as usize].last_child;
        self.nodes.push(Node { kind, parent, first_child: NIL, last_child: NIL, next: NIL, prev: last });
        if last == NIL {
            self.nodes[parent as usize].first_child = id;
        } else {
            self.nodes[last as usize].next = id;
        }
        self.nodes[parent as usize].last_child = id;
        id
    }

    #[inline]
    pub fn node(&self, id: u32) -> &Node<'a> {
        &self.nodes[id as usize]
    }

    pub fn is_element(&self, id: u32) -> bool {
        matches!(self.node(id).kind, Kind::Element { .. })
    }

    /// Next node after `cur` in document order, *not* entering `cur`'s own subtree.
    pub fn next_after(&self, cur: u32, root: u32) -> u32 {
        let mut c = cur;
        loop {
            if c == root {
                return NIL;
            }
            let nx = self.node(c).next;
            if nx != NIL {
                return nx;
            }
            c = self.node(c).parent;
            if c == NIL {
                return NIL;
            }
        }
    }

    /// Next node in document order inside the subtree of `root` (exclusive of `root` itself at the start).
    pub fn next_preorder(&self, cur: u32, root: u32) -> u32 {
        let n = self.node(cur);
        if n.first_child != NIL {
            return n.first_child;
        }
        let mut c = cur;
        loop {
            if c == root {
                return NIL;
            }
            let nx = self.node(c).next;
            if nx != NIL {
                return nx;
            }
            c = self.node(c).parent;
            if c == NIL {
                return NIL;
            }
        }
    }
}
