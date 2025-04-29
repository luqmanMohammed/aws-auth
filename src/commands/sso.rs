use crate::cmd::Sso;

#[derive(Debug)]
pub enum Error {}

impl std::error::Error for Error {}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unimplemented!()
    }
}

pub fn exec_sso(subcommand: Sso) -> Result<(), Error> {
    unimplemented!()
}
