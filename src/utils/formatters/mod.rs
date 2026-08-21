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
