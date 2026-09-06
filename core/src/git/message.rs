//! Commit messages for automatic commits, derived from what changed in the
//! working directory so they read like a changelog: "New note: recipes",
//! "Note recipes edited", "Note recipes renamed to cooking".

/// One changed path, classified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Added(String),
    Modified(String),
    Deleted(String),
    /// `(from, to)`
    Renamed(String, String),
}

/// Is this path a note (as opposed to an attachment or other file)?
fn is_note(path: &str) -> bool {
    crate::has_md_extension(path)
}

/// Display name of a path: notes lose their `.md`, other files keep their name.
fn display_name(path: &str) -> &str {
    if is_note(path) {
        &path[..path.len() - 3]
    } else {
        path
    }
}

/// One line describing a single change.
pub fn describe_change(change: &Change) -> String {
    let path = match change {
        Change::Added(p) | Change::Modified(p) | Change::Deleted(p) => p,
        Change::Renamed(from, _) => from,
    };
    let kind = if is_note(path) { "Note" } else { "File" };
    match change {
        Change::Added(p) => {
            if is_note(p) {
                format!("New note: {}", display_name(p))
            } else {
                format!("New file: {p}")
            }
        }
        Change::Modified(p) => format!("{kind} {} edited", display_name(p)),
        Change::Deleted(p) => format!("{kind} {} deleted", display_name(p)),
        Change::Renamed(from, to) => {
            format!(
                "{kind} {} renamed to {}",
                display_name(from),
                display_name(to)
            )
        }
    }
}

/// Build a full commit message (title line, blank line, body) for a set of
/// changes. A single change becomes the title; several changes get the first
/// one as the title plus a "(+N more)" marker, with every change listed in the
/// body. Returns `None` when there is nothing to describe.
pub fn commit_message(changes: &[Change]) -> Option<String> {
    let first = changes.first()?;
    let title = describe_change(first);
    if changes.len() == 1 {
        return Some(title);
    }
    let more = changes.len() - 1;
    let mut msg = format!("{title} (+{more} more)\n\n");
    for change in changes {
        msg.push_str("* ");
        msg.push_str(&describe_change(change));
        msg.push('\n');
    }
    Some(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_changes_have_specific_titles() {
        assert_eq!(
            commit_message(&[Change::Added("recipes.md".into())]).unwrap(),
            "New note: recipes"
        );
        assert_eq!(
            commit_message(&[Change::Modified("work/standup.md".into())]).unwrap(),
            "Note work/standup edited"
        );
        assert_eq!(
            commit_message(&[Change::Deleted("old.md".into())]).unwrap(),
            "Note old deleted"
        );
        assert_eq!(
            commit_message(&[Change::Renamed("untitled_1.md".into(), "recipes.md".into())])
                .unwrap(),
            "Note untitled_1 renamed to recipes"
        );
    }

    #[test]
    fn non_note_files_are_called_files() {
        assert_eq!(
            commit_message(&[Change::Added("img/photo.png".into())]).unwrap(),
            "New file: img/photo.png"
        );
        assert_eq!(
            commit_message(&[Change::Modified(".gitignore".into())]).unwrap(),
            "File .gitignore edited"
        );
    }

    #[test]
    fn dotted_note_names_keep_their_dots() {
        assert_eq!(
            describe_change(&Change::Modified("sprint-q2.6.md".into())),
            "Note sprint-q2.6 edited"
        );
    }

    #[test]
    fn several_changes_get_a_summary_title_and_a_body() {
        let msg = commit_message(&[
            Change::Modified("a.md".into()),
            Change::Added("b.md".into()),
            Change::Deleted("c.md".into()),
        ])
        .unwrap();
        let mut lines = msg.lines();
        assert_eq!(lines.next(), Some("Note a edited (+2 more)"));
        assert_eq!(lines.next(), Some(""));
        assert_eq!(lines.next(), Some("* Note a edited"));
        assert_eq!(lines.next(), Some("* New note: b"));
        assert_eq!(lines.next(), Some("* Note c deleted"));
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn no_changes_no_message() {
        assert_eq!(commit_message(&[]), None);
    }
}
