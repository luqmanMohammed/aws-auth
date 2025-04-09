use super::TabularFormatter;

pub struct JsonFormatter<'a, C> {
    _phantom: std::marker::PhantomData<C>,
    omit_fields: Vec<&'a str>,
    no_headers: bool,
}

impl<'a, C> JsonFormatter<'a, C>
where
    C: std::string::ToString,
    C: serde::Serialize,
{
    pub fn new(omit_fields: Vec<&'a str>, no_headers: bool) -> Self {
        Self {
            _phantom: std::marker::PhantomData {},
            omit_fields,
            no_headers,
        }
    }
}

impl<C> TabularFormatter<C> for JsonFormatter<'_, C>
where
    C: std::string::ToString,
    C: serde::Serialize,
{
    type Error = serde_json::Error;
    fn format<'r, I, O>(&self, headers: &'r [&'r str], rows: O) -> Result<String, Self::Error>
    where
        C: 'r,
        I: IntoIterator<Item = C> + 'r,
        O: IntoIterator<Item = I> + 'r,
    {
        if self.no_headers {
            let mut json_rows = Vec::new();
            for row in rows {
                let mut json_row = Vec::new();
                for (i, field) in row.into_iter().enumerate() {
                    if self.omit_fields.contains(&headers[i]) {
                        continue;
                    }
                    json_row.push(serde_json::to_value(field)?);
                }
                json_rows.push(json_row);
            }
            serde_json::to_string(&json_rows)
        } else {
            let mut json_rows = Vec::new();
            for row in rows {
                let mut json_row = serde_json::json!({});
                for (i, field) in row.into_iter().enumerate() {
                    if self.omit_fields.contains(&headers[i]) {
                        continue;
                    }
                    json_row[headers[i]] = serde_json::to_value(field)?;
                }
                json_rows.push(json_row);
            }
            serde_json::to_string(&json_rows)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_formatter() {
        let formatter = JsonFormatter::new(vec!["age"], false);
        let rows = [&["Alice", "30"], &["Bob", "25"]];
        let output = formatter.format(&["name", "age"], rows);
        let expected_output = json!([
            {"name": "Alice"},
            {"name": "Bob"},
        ]);
        assert_eq!(output.unwrap(), expected_output.to_string());

        let formatter = JsonFormatter::new(vec!["age"], true);
        let rows = [&["Alice", "30"], &["Bob", "25"]];
        let output = formatter.format(&["name", "age"], rows);
        let expected_output = json!([["Alice"], ["Bob"],]);
        assert_eq!(output.unwrap(), expected_output.to_string());
    }
}
