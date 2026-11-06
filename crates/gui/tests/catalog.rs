//! Keep the catalog and marked source messages in sync, including plurals.
use std::collections::HashSet;
use std::path::Path;

fn literal(text: &str) -> (String, &str) {
    // Both PO and the messages here use quoted strings with escaped quotes.
    let mut escaped = false;
    for (i, c) in text.char_indices().skip(1) {
        if c == '"' && !escaped {
            return (
                serde_json::from_str(&text[..=i]).expect("valid message literal"),
                &text[i + 1..],
            );
        }
        escaped = c == '\\' && !escaped;
    }
    panic!("unterminated message: {text}");
}

fn quoted(text: &str) -> String {
    literal(text).0
}

fn catalog_ids(po: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = None;
    for line in po.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("msgid ") {
            if let Some(id) = current.take() {
                result.push(id);
            }
            current = Some(quoted(rest));
        } else if let Some(rest) = line.strip_prefix("msgid_plural ") {
            if let Some(id) = current.take() {
                result.push(id);
            }
            current = Some(quoted(rest));
        } else if line.starts_with('"') {
            if let Some(id) = &mut current {
                id.push_str(&quoted(line));
            }
        } else if line.starts_with("msgstr")
            && let Some(id) = current.take()
        {
            result.push(id);
        }
    }
    if let Some(id) = current {
        result.push(id);
    }
    result.into_iter().filter(|id| !id.is_empty()).collect()
}

fn source_ids_in(text: &str) -> HashSet<String> {
    let mut result = HashSet::new();
    for call in text.split("@tr(").skip(1) {
        let (first, rest) = literal(call.trim_start());
        let (message, rest) = match rest.trim_start().strip_prefix("=>") {
            Some(message) => literal(message.trim_start()),
            None => (first, rest),
        };
        result.insert(message);
        if let Some(plural) = rest.trim_start().strip_prefix('|') {
            result.insert(quoted(plural.trim_start()));
        }
    }
    result
}

fn source_ids(dir: &Path) -> HashSet<String> {
    let mut result = HashSet::new();
    for file in std::fs::read_dir(dir).unwrap() {
        let path = file.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "slint") {
            result.extend(source_ids_in(&std::fs::read_to_string(path).unwrap()));
        }
    }
    result
}

fn check_catalog(language: &str, complete: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let po = std::fs::read_to_string(root.join(format!(
        "translations/{language}/LC_MESSAGES/hatchzip-gui.po"
    )))
    .unwrap();
    let sources = source_ids(&root.join("ui"));
    let catalog: HashSet<_> = catalog_ids(&po).into_iter().collect();
    let stale: Vec<_> = catalog.iter().filter(|id| !sources.contains(*id)).collect();
    assert!(
        stale.is_empty(),
        "{language} catalog contains removed source messages: {stale:?}"
    );
    if !complete {
        return;
    }
    let missing: Vec<_> = sources.difference(&catalog).collect();
    assert!(
        missing.is_empty(),
        "{language} catalog is missing messages: {missing:?}"
    );
}

/// French covers every window, the browser and the add dialog included.
#[test]
fn french_catalog_matches_marked_messages() {
    check_catalog("fr", true);
}

/// Hindi covers the extraction windows only; the browser and the add dialog
/// fall back to English. It must still hold nothing that was removed.
#[test]
fn hindi_catalog_has_no_stale_messages() {
    check_catalog("hi", false);
}

#[test]
fn catalog_reader_handles_multiline_and_escaped_messages() {
    let po = "msgid \"\"\n\"Hello \\\"world\\\"\"\nmsgstr \"translation\"\n";
    assert_eq!(catalog_ids(po), ["Hello \"world\""]);
}

#[test]
fn plural_messages_are_checked_on_both_sides() {
    let po = "msgid \"one\"\nmsgid_plural \"many\"\nmsgstr[0] \"a\"\nmsgstr[1] \"b\"\n";
    assert_eq!(catalog_ids(po), ["one", "many"]);
    assert_eq!(
        source_ids_in("@tr(\"one\" | \"many\" % n)"),
        HashSet::from(["one".into(), "many".into()])
    );
    assert_eq!(
        source_ids_in("@tr(\"context\" => \"one\" | \"many\" % n)"),
        HashSet::from(["one".into(), "many".into()])
    );
}
