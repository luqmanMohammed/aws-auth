use std::collections::HashMap;

use super::TabularFormatter;

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

        let mut output = String::new();
        let filtered_headers = headers
            .iter()
            .filter(|v| !self.omit_fields.contains(v))
            .collect::<Vec<_>>();

        let vrows: Vec<Vec<C>> = rows.into_iter().map(|r| r.into_iter().collect()).collect();

        let field_longest: HashMap<&str, usize> = filtered_headers
            .iter()
            .map(|header| {
                let header_max_len = header.len();
                let field_max_len = vrows
                    .iter()
                    .map(|row| row[*header_i.get(*header).unwrap()].to_string().len())
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
