//! App links: `shy://<workspace id>/<kind>/<id>`, naming the workspace their
//! target is in, so a link opened anywhere knows where to go. Links written
//! before that, `shy://<kind>/<id>`, mean the workspace they're read in.

pub(crate) const APP_LINK: &str = "shy://";
const WORKSPACE_ID_LEN: usize = 16;

/// A parsed app link; `rest` is what follows `<kind>/`, the id and anything
/// after it.
#[derive(Debug, PartialEq)]
pub(crate) struct AppLink<'a> {
    /// None for a link written before links named their workspace.
    pub workspace: Option<&'a str>,
    pub kind: &'a str,
    pub rest: &'a str,
}

impl AppLink<'_> {
    /// Whether the link points into `workspace_id`, the one it's read in.
    pub(crate) fn is_in(&self, workspace_id: &str) -> bool {
        self.workspace.is_none_or(|workspace| workspace == workspace_id)
    }
}

/// The app link starting `text`, if it is one.
pub(crate) fn parse(text: &str) -> Option<AppLink<'_>> {
    let after = text.strip_prefix(APP_LINK)?;
    let (first, rest) = after.split_once('/')?;
    if !is_workspace_id(first) {
        return Some(AppLink {
            workspace: None,
            kind: first,
            rest,
        });
    }
    let (kind, rest) = rest.split_once('/')?;
    Some(AppLink {
        workspace: Some(first),
        kind,
        rest,
    })
}

/// A link to `id` of `kind` in `workspace_id`. A workspace whose id isn't
/// shaped like `new_random_id`'s (one joined with an odd invite) gets an
/// older link, which still means the workspace it's read in.
pub fn app_link(workspace_id: &str, kind: &str, id: &str) -> String {
    match is_workspace_id(workspace_id) {
        true => format!("{APP_LINK}{workspace_id}/{kind}/{id}"),
        false => format!("{APP_LINK}{kind}/{id}"),
    }
}

fn is_workspace_id(text: &str) -> bool {
    text.len() == WORKSPACE_ID_LEN
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: &str = "0123456789abcdef";

    #[test]
    fn a_link_names_its_workspace_and_an_older_one_names_none() {
        assert_eq!(
            parse(&app_link(HERE, "page", "p1")),
            Some(AppLink {
                workspace: Some(HERE),
                kind: "page",
                rest: "p1"
            })
        );
        assert_eq!(
            parse("shy://message/p:ab/cd and more"),
            Some(AppLink {
                workspace: None,
                kind: "message",
                rest: "p:ab/cd and more"
            })
        );
        assert_eq!(parse("https://a.example/page/p1"), None);
    }

    #[test]
    fn an_older_link_is_in_the_workspace_it_is_read_in() {
        assert!(parse("shy://page/p1").unwrap().is_in(HERE));
        assert!(parse(&app_link(HERE, "page", "p1")).unwrap().is_in(HERE));
        assert!(
            !parse(&app_link("fedcba9876543210", "page", "p1"))
                .unwrap()
                .is_in(HERE)
        );
    }

    #[test]
    fn a_workspace_with_an_odd_id_gets_links_that_still_resolve() {
        let link = app_link("workspace-id", "page", "p1");
        assert_eq!(link, "shy://page/p1");
        assert!(parse(&link).unwrap().is_in("workspace-id"));
    }

    #[test]
    fn only_sixteen_lowercase_hex_characters_name_a_workspace() {
        for not_a_workspace in [
            "0123456789ABCDEF",
            "0123456789abcde",
            "0123456789abcdef0",
            "member",
        ] {
            let link = format!("shy://{not_a_workspace}/page/p1");
            assert_eq!(parse(&link).unwrap().workspace, None, "{link}");
        }
    }
}
