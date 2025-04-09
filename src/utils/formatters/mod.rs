pub mod json;
pub mod text;

pub trait TabularFormatter<C>
where
    C: std::string::ToString,
{
    type Error: std::error::Error + 'static;
    fn format<'r, I, O>(&self, headers: &'r [&'r str], rows: O) -> Result<String, Self::Error>
    where
        C: 'r,
        I: IntoIterator<Item = C> + 'r,
        O: IntoIterator<Item = I> + 'r;
}
