pub const LEVELS: usize = 16;
pub const LINES: usize = 1600;
pub const VISIT_CAP: usize = 100_000;

pub trait Tree {
    type Node;
    fn name(&self, node: &Self::Node) -> String;
    fn value(&self, node: &Self::Node) -> String;
    fn children(&self, node: &Self::Node) -> Vec<Self::Node>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Read {
    pub text: String,
    pub lines: usize,
    pub visited: usize,
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| {
            if matches!(c, '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

pub fn line(name: &str, value: &str) -> Option<String> {
    let name = one_line(name);
    let value = one_line(value);
    if name.is_empty() && value.is_empty() {
        None
    } else if value.is_empty() {
        Some(name)
    } else {
        Some(format!("{name}\t{value}"))
    }
}

pub fn walk<T: Tree>(tree: &T, root: &T::Node) -> Read {
    let mut read = Read::default();
    visit(tree, root, 1, &mut read);
    read
}

fn visit<T: Tree>(tree: &T, node: &T::Node, level: usize, read: &mut Read) {
    if read.lines >= LINES || read.visited >= VISIT_CAP {
        return;
    }
    read.visited += 1;
    if let Some(line) = line(&tree.name(node), &tree.value(node)) {
        if read.lines > 0 {
            read.text.push('\n');
        }
        read.text.push_str(&line);
        read.lines += 1;
    }
    if level >= LEVELS {
        return;
    }
    for child in tree.children(node) {
        if read.lines >= LINES || read.visited >= VISIT_CAP {
            return;
        }
        visit(tree, &child, level + 1, read);
    }
}
