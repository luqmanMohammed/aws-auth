pub mod json;
pub mod text;

/// Names in `omit_fields` that match no column, so a misspelling is reported rather than
/// silently leaving every column in place.
pub fn unknown_fields<'a>(headers: &[&str], omit_fields: &[&'a str]) -> Vec<&'a str> {
    let known: Vec<String> = headers.iter().map(|h| normalize_field(h)).collect();
    omit_fields
        .iter()
        .copied()
        .filter(|field| !known.contains(&normalize_field(field)))
        .collect()
}

/// Each format labels its columns differently ("accountId" against "Account Id"), so omitted
/// fields are matched on this form to keep one spelling working across both.
fn normalize_field(field: &str) -> String {
    field
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

pub trait TabularFormatter<C>
where
    C: std::fmt::Display,
{
    type Error: std::error::Error + 'static;
    fn format<'r, I, O>(&self, headers: &'r [&'r str], rows: O) -> Result<String, Self::Error>
    where
        C: 'r,
        I: IntoIterator<Item = C> + 'r,
        O: IntoIterator<Item = I> + 'r;
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalising_ignores_case_and_spacing() {
        assert_eq!(normalize_field("accountId"), "accountid");
        assert_eq!(normalize_field("Account Id"), "accountid");
        assert_eq!(normalize_field("ACCOUNT ID"), "accountid");
        assert_eq!(normalize_field(" account  id "), "accountid");
    }

    #[test]
    fn the_two_header_spellings_normalise_together() {
        // The json and text column labels for the same field must collapse to one form, or
        // --omit-fields works for only one output format.
        for (json, text) in [
            ("accountId", "Account Id"),
            ("accountName", "Account Name"),
            ("accountEmail", "Account Email"),
            ("roleName", "Role Name"),
            ("alias", "Alias"),
            ("role", "Role"),
        ] {
            assert_eq!(
                normalize_field(json),
                normalize_field(text),
                "{json} and {text} should match"
            );
        }
    }

    #[test]
    fn known_fields_are_accepted_in_either_spelling() {
        let headers = ["alias", "accountId", "role"];
        for name in [
            "role",
            "Role",
            "ROLE",
            "accountId",
            "account id",
            "Account Id",
        ] {
            assert!(
                unknown_fields(&headers, &[name]).is_empty(),
                "{name} should be recognised"
            );
        }
    }

    #[test]
    fn unknown_fields_are_reported() {
        let headers = ["alias", "accountId", "role"];
        assert_eq!(unknown_fields(&headers, &["nope"]), vec!["nope"]);
        assert_eq!(
            unknown_fields(&headers, &["role", "nope", "alias"]),
            vec!["nope"],
            "only the unrecognised name should be returned"
        );
        assert!(unknown_fields(&headers, &[]).is_empty());
    }
}
