//! Translation helpers. Strings shown to the reader are translated with
//! `gettextrs::gettext` and `ngettext`; `po/` holds the catalog setup.
//!
//! Placeholders in translatable strings are named, as in `{name}`, and
//! filled in with `str::replace`, so translators can move them.

/// Marks `msgid` for translation without translating it, for strings kept
/// in constants. Pass it to `gettext` where it is shown.
pub const fn gettext_noop(msgid: &str) -> &str {
    msgid
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    /// The files under `folder`, recursively, relative to `root`.
    fn files(root: &Path, folder: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut folders = vec![root.join(folder)];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    folders.push(path);
                } else {
                    let relative = path.strip_prefix(root).unwrap();
                    found.push(relative.to_string_lossy().into_owned());
                }
            }
        }
        found
    }

    /// Whether `source` has a string literal marked for translation.
    fn has_marked_strings(source: &str) -> bool {
        ["gettext(", "gettext_noop(", "_("].iter().any(|keyword| {
            source.match_indices(keyword).any(|(start, _)| {
                source[start + keyword.len()..]
                    .trim_start()
                    .starts_with('"')
            })
        })
    }

    #[test]
    fn every_file_with_marked_strings_is_in_potfiles() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let potfiles = fs::read_to_string(root.join("po/POTFILES.in")).unwrap();
        let listed: Vec<&str> = potfiles
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        for file in &listed {
            assert!(
                root.join(file).is_file(),
                "po/POTFILES.in lists {file}, which doesn't exist"
            );
        }

        let mut sources = files(&root, "msg-viewer/src");
        sources.extend(files(&root, "data/resources/ui"));
        for file in sources {
            // This file's tests quote the keywords.
            let checked = file.ends_with(".rs") || file.ends_with(".blp");
            if !checked || file == "msg-viewer/src/i18n.rs" {
                continue;
            }
            let source = fs::read_to_string(root.join(&file)).unwrap();
            assert!(
                !has_marked_strings(&source) || listed.contains(&file.as_str()),
                "{file} has strings marked for translation; add it to po/POTFILES.in"
            );
        }
    }

    #[test]
    fn finds_marked_strings() {
        assert!(has_marked_strings(r#"gettext("Quit")"#));
        assert!(has_marked_strings("ngettext(\n    \"{count} Attachment\","));
        assert!(has_marked_strings(r#"label: _("To");"#));
        assert!(!has_marked_strings("gettext(title)"));
        assert!(!has_marked_strings("const fn gettext_noop(msgid: &str)"));
    }
}
