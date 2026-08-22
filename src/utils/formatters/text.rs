use std::collections::HashMap;

use super::{TabularFormatter, normalize_field};

pub struct TextFormatter<'a> {
    omit_fields: Vec<&'a str>,
    no_headers: bool,
    separator: &'a str,
}

impl<'a> TextFormatter<'a> {
    pub fn new(omit_fields: Vec<&'a str>, no_headers: bool, separator: &'a str) -> Self {
        Self {
            omit_fields,
            no_headers,
            separator,
        }
    }
}

impl<C> TabularFormatter<C> for TextFormatter<'_>
where
    C: std::fmt::Display,
{
    type Error = std::io::Error;
    fn format<'r, I, O>(&self, headers: &'r [&'r str], rows: O) -> Result<String, Self::Error>
    where
        C: 'r,
        I: IntoIterator<Item = C> + 'r,
        O: IntoIterator<Item = I> + 'r,
    {
        let header_i: HashMap<&str, usize> =
            headers.iter().enumerate().map(|(i, v)| (*v, i)).collect();

        let omitted: Vec<String> = self
            .omit_fields
            .iter()
            .map(|v| normalize_field(v))
            .collect();

        let mut output = String::new();
        let filtered_headers = headers
            .iter()
            .filter(|v| !omitted.contains(&normalize_field(v)))
            .collect::<Vec<_>>();

        let vrows: Vec<Vec<C>> = rows.into_iter().map(|r| r.into_iter().collect()).collect();

        let field_longest: HashMap<&str, usize> = filtered_headers
            .iter()
            .map(|header| {
                let header_max_len = header.len();
                let field_max_len = vrows
                    .iter()
                    .map(
                        |row| match header_i.get(*header).and_then(|i| row.get(*i)) {
                            Some(field) => field.to_string().len(),
                            None => 0,
                        },
                    )
                    .max()
                    .unwrap_or(0);
                (**header, std::cmp::max(header_max_len, field_max_len))
            })
            .collect();

        if !self.no_headers {
            for (i, header) in filtered_headers.iter().enumerate() {
                let h_padding = field_longest.get(*header).unwrap() - header.len();
                output.push_str("\x1b[1m");
                output.push_str(header);
                output.push_str("\x1b[0m");
                if i != filtered_headers.len() - 1 {
                    output.push_str(&" ".repeat(h_padding));
                    output.push_str(self.separator);
                }
            }
            if !filtered_headers.is_empty() {
                let field_max_sum: usize = field_longest.values().sum::<usize>()
                    + (filtered_headers.len() - 1) * self.separator.len();
                output.push('\n');
                output.push_str(&"-".repeat(field_max_sum));
                output.push('\n');
            }
        }

        'outer: for (ri, row) in vrows.iter().enumerate() {
            for (i, header) in filtered_headers.iter().enumerate() {
                let h_index = *header_i.get(*header).unwrap();
                let field = row.get(h_index).map(|v| v.to_string()).unwrap_or_default();
                output.push_str(&field);
                if i != filtered_headers.len() - 1 {
                    let f_padding = *field_longest.get(*header).unwrap() - field.len();
                    output.push_str(&" ".repeat(f_padding));
                    output.push_str(self.separator);
                } else if ri == vrows.len() - 1 {
                    break 'outer;
                } else {
                    output.push('\n');
                }
            }
        }

        Ok(output)
    }
}

// Tests were written by AI (Claude Opus 5), not reviewed by Author
#[cfg(test)]
mod tests {
    use super::*;

    fn render(omit: Vec<&str>, headers: &[&str], rows: Vec<Vec<&str>>) -> String {
        TextFormatter::new(omit, true, " | ")
            .format(headers, rows)
            .expect("the text formatter does not fail")
    }

    #[test]
    fn renders_every_column_separated() {
        let out = render(vec![], &["a", "b"], vec![vec!["1", "2"]]);
        assert_eq!(out, "1 | 2");
    }

    #[test]
    fn omitting_a_column_drops_it() {
        let out = render(vec!["b"], &["a", "b"], vec![vec!["1", "2"]]);
        assert_eq!(out, "1");
    }

    #[test]
    fn omitting_matches_regardless_of_case_or_spacing() {
        let out = render(
            vec!["ACCOUNT ID"],
            &["alias", "accountId"],
            vec![vec!["x", "1"]],
        );
        assert_eq!(out, "x");
    }

    #[test]
    fn columns_are_padded_to_the_widest_value() {
        let out = render(
            vec![],
            &["a", "b"],
            vec![vec!["short", "1"], vec!["muchlonger", "2"]],
        );
        for line in out.lines() {
            assert!(
                line.contains(" | "),
                "each row keeps the separator: {line:?}"
            );
        }
        let widths: Vec<usize> = out
            .lines()
            .map(|line| line.split(" | ").next().unwrap().len())
            .collect();
        assert_eq!(
            widths,
            vec![10, 10],
            "first column padded to the widest value"
        );
    }

    #[test]
    fn empty_values_do_not_break_alignment() {
        let out = render(vec![], &["a", "b"], vec![vec!["", ""], vec!["xx", "yy"]]);
        assert_eq!(out.lines().count(), 2);
    }

    #[test]
    fn a_row_shorter_than_the_headers_does_not_panic() {
        // Column widths used to be measured by indexing directly into the row.
        let out = render(vec![], &["a", "b", "c"], vec![vec!["only-one"]]);
        assert!(out.contains("only-one"));
    }

    #[test]
    fn headers_are_included_when_not_suppressed() {
        let out = TextFormatter::new(vec![], false, " | ")
            .format(&["Alias", "Role"], vec![vec!["x", "y"]])
            .expect("the text formatter does not fail");
        assert!(
            out.contains("Alias"),
            "header row should be present: {out:?}"
        );
        assert!(
            out.contains('-'),
            "a rule should separate headers from rows"
        );
    }

    #[test]
    fn no_rows_still_renders_headers() {
        let out = TextFormatter::new(vec![], false, " | ")
            .format(&["Alias"], Vec::<Vec<&str>>::new())
            .expect("the text formatter does not fail");
        assert!(out.contains("Alias"));
    }
}
