//! Builds a tree incrementally from flat `(path, size)` records.
//! Used by remote backends (`find`, `du`, `rclone lsjson`).

use super::local::SMALL_FILES_NAME;
use crate::tree::{Kind, Tree, F_DONE};
use std::collections::HashMap;

pub struct PathTreeBuilder {
    /// Relative dir path -> node id.
    dirs: HashMap<String, u32>,
    /// Dir node id -> its small files node.
    small: HashMap<u32, u32>,
    pub keep_min: u64,
}

/// `rel` with its outer slashes trimmed, if every component names one real
/// entry: not empty, `.` or `..`, and no NUL. Remote listings come from
/// another machine, or from a bucket whose keys can be any string, and a
/// `..` would make the tree (and so a delete) point somewhere other than what
/// is shown.
fn safe_rel(rel: &str) -> Option<&str> {
    let rel = rel.trim_matches('/');
    let ok = rel.is_empty() || rel.split('/').all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains('\0'));
    ok.then_some(rel)
}

impl PathTreeBuilder {
    pub fn new(root: u32, keep_min: u64) -> Self {
        let mut dirs = HashMap::new();
        dirs.insert(String::new(), root);
        PathTreeBuilder { dirs, small: HashMap::new(), keep_min }
    }

    /// Makes sure the directory `rel` (slash separated, relative) exists.
    /// Returns `None`, adding nothing, for an unsafe path (see [`safe_rel`]).
    pub fn ensure_dir(&mut self, tree: &mut Tree, rel: &str) -> Option<u32> {
        Some(self.dir(tree, safe_rel(rel)?))
    }

    fn dir(&mut self, tree: &mut Tree, rel: &str) -> u32 {
        if let Some(id) = self.dirs.get(rel) {
            return *id;
        }
        let (parent_rel, name) = match rel.rfind('/') {
            Some(i) => (&rel[..i], &rel[i + 1..]),
            None => ("", rel),
        };
        let parent = self.dir(tree, parent_rel);
        let id = tree.add_child(parent, name, Kind::Dir, 0, 0);
        tree.get_mut(id).flags |= F_DONE;
        self.dirs.insert(rel.to_string(), id);
        id
    }

    pub fn add_file(&mut self, tree: &mut Tree, rel: &str, size: u64) {
        let Some(rel) = safe_rel(rel).filter(|r| !r.is_empty()) else { return };
        let (parent_rel, name) = match rel.rfind('/') {
            Some(i) => (&rel[..i], &rel[i + 1..]),
            None => ("", rel),
        };
        let parent = self.dir(tree, parent_rel);
        if size >= self.keep_min {
            tree.add_child_sized(parent, name, Kind::File, size, 1);
        } else {
            let small = match self.small.get(&parent) {
                Some(s) => *s,
                None => {
                    let s = tree.add_child(parent, SMALL_FILES_NAME, Kind::SmallFiles, 0, 0);
                    self.small.insert(parent, s);
                    s
                }
            };
            tree.add_size(small, size as i64, 1);
        }
    }

    /// For `du`-style records (post-order cumulative directory totals): sets the
    /// directory's *own* files size as the difference with its children.
    pub fn add_du_dir(&mut self, tree: &mut Tree, rel: &str, cumulative: u64) {
        let Some(id) = self.ensure_dir(tree, rel) else { return };
        let current = tree.get(id).size;
        if cumulative > current {
            let small = tree.add_child(id, SMALL_FILES_NAME, Kind::SmallFiles, 0, 0);
            tree.add_size(small, (cumulative - current) as i64, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Source;

    #[test]
    fn builds_from_paths() {
        let mut t = Tree::new("/", "/", Source::Ssh { host: "x".into() });
        let mut b = PathTreeBuilder::new(0, 100);
        b.add_file(&mut t, "a/b/c.bin", 1000);
        b.add_file(&mut t, "a/x.txt", 10);
        b.add_file(&mut t, "a/y.txt", 20);
        assert_eq!(t.get(0).size, 1030);
        let (a, _) = t.find_path("/a");
        assert_eq!(t.child_count(a), 2); // b + small files
    }

    #[test]
    fn ignores_paths_that_leave_the_root() {
        let mut t = Tree::new("/srv", "/srv", Source::Rclone { remote: "s3:b".into() });
        let mut b = PathTreeBuilder::new(0, 0);
        for bad in ["../etc/passwd", "a/../../b", "a/./b", "a//b", "..", ".", "", "/", "a/\0b"] {
            b.add_file(&mut t, bad, 1000);
            // "" and "/" are the root itself, which du does report
            if !bad.trim_matches('/').is_empty() {
                b.add_du_dir(&mut t, bad, 1000);
                assert!(b.ensure_dir(&mut t, bad).is_none(), "{bad:?}");
            }
        }
        assert_eq!(t.get(0).size, 0);
        assert_eq!(t.child_count(0), 0);
        // odd but real names are kept
        b.add_file(&mut t, "/a/..b/c..", 10);
        b.add_file(&mut t, "dir/with\\backslash and spaces", 5);
        assert_eq!(t.get(0).size, 15);
        assert!(t.find_path("/srv/a/..b/c..").1);
    }

    #[test]
    fn du_records() {
        let mut t = Tree::new("/", "/", Source::Ssh { host: "x".into() });
        let mut b = PathTreeBuilder::new(0, 100);
        b.add_du_dir(&mut t, "a/b", 300);
        b.add_du_dir(&mut t, "a", 500);
        b.add_du_dir(&mut t, "", 600);
        assert_eq!(t.get(0).size, 600);
        let (a, _) = t.find_path("/a");
        assert_eq!(t.get(a).size, 500);
    }
}
